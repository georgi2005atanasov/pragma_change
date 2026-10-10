use super::super::{
    audit::audit,
    auth::{admin, authenticate},
    contracts::Batch,
    error::{Result, bad, db_error},
    privacy::{hash, sanitize},
    state::App,
};
use super::models::load_model;
use crate::server::repositories::collection as repository;
use axum::http::HeaderMap;
use pragma_behavior::{Event, History, Model, Outcome, evaluate};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
pub(crate) async fn record(app: &App, headers: &HeaderMap, input: Batch) -> Result<Value> {
    if input.events.is_empty() || input.events.len() > 100 {
        return Err(bad("send 1–100 events"));
    }
    let application = input.events[0].application_id;
    let mut tx = app.db.begin().await.map_err(db_error)?;
    repository::lock_application_shared()
        .bind(application.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    let (policy, active) = authenticate(app, headers, application).await?;
    let model: Option<Model> = load_model(app, active).await?;
    let mut events = vec![];
    for event in input.events {
        event.validate().map_err(bad)?;
        if event.application_id != application {
            return Err(bad("batch application mismatch"));
        }
        events.push(sanitize(app, &policy, event));
    }
    // Stable lock order prevents multi-partition batch deadlocks.
    events.sort_by_key(|e| {
        (
            e.partition(),
            e.timestamp_ms,
            e.sequence_number.unwrap_or(u64::MAX),
            e.event_id,
        )
    });
    let mut accepted = 0;
    for event in events {
        let partition = event.partition();
        let content = serde_json::to_value(&event).map_err(bad)?;
        let digest = hash(serde_json::to_vec(&event).map_err(bad)?);
        repository::lock_partition()
            .bind(format!("{application}:{partition}"))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let existing: Option<String> = repository::event_hash()
            .bind(application)
            .bind(event.event_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        if let Some(old) = existing {
            if old != digest {
                return Err(bad("event ID reused with different content"));
            }
            continue;
        }
        if event.outcome != Outcome::Attempted
            && let Some(correlation) = event.correlation_id
        {
            let context: Option<Value> = repository::attempt_context()
                .bind(application)
                .bind(correlation)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
            let attempt: Event = serde_json::from_value(
                context.ok_or_else(|| bad("correlation_id must identify a recorded evaluation"))?,
            )
            .map_err(bad)?;
            if attempt.partition() != event.partition()
                || attempt.action != event.action
                || attempt.resource != event.resource
            {
                return Err(bad("outcome does not match its evaluated attempt"));
            }
        }
        let old: Option<Value> = repository::history_for_update()
            .bind(application)
            .bind(&partition)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        let mut history: History = old
            .map(serde_json::from_value)
            .transpose()
            .map_err(bad)?
            .unwrap_or_default();
        if event.outcome != Outcome::Attempted {
            let decision = evaluate(model.as_ref(), &policy, &event, &history);
            if event.sequence_start {
                history = History {
                    complete: true,
                    ..History::default()
                };
            }
            history.observe(&event);
            if let Some(surprise) = decision.surprise {
                history.surprises.push(surprise);
                if history.surprises.len() > 128 {
                    history.surprises.remove(0);
                }
            }
            repository::save_history()
                .bind(application)
                .bind(&partition)
                .bind(json!(history))
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        repository::insert_event()
            .bind(application)
            .bind(event.event_id)
            .bind(partition)
            .bind(digest)
            .bind(content)
            .bind(event.timestamp_ms as i64)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        accepted += 1;
    }
    tx.commit().await.map_err(db_error)?;
    Ok(json!({"accepted":accepted}))
}
pub(crate) async fn check(app: &App, headers: &HeaderMap, event: Event) -> Result<Value> {
    event.validate().map_err(bad)?;
    if event.outcome != Outcome::Attempted {
        return Err(bad("evaluate expects an attempted action"));
    }
    let mut tx = app.db.begin().await.map_err(db_error)?;
    repository::lock_application_shared()
        .bind(event.application_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    let (policy, active) = authenticate(app, headers, event.application_id).await?;
    let event = sanitize(app, &policy, event);
    let digest = hash(serde_json::to_vec(&event).map_err(bad)?);
    let model = load_model(app, active).await?;
    repository::lock_partition()
        .bind(format!("{}:{}", event.application_id, event.partition()))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    if let Some(row) = repository::decision()
        .bind(event.application_id)
        .bind(event.event_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
    {
        if row.get::<String, _>("content_hash") != digest {
            return Err(bad("evaluation ID reused with different content"));
        }
        return Ok(row.get("decision"));
    }
    let history: Option<Value> = repository::history()
        .bind(event.application_id)
        .bind(event.partition())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
    let history = history
        .map(serde_json::from_value)
        .transpose()
        .map_err(bad)?
        .unwrap_or_default();
    let result = json!(evaluate(model.as_ref(), &policy, &event, &history));
    repository::insert_decision()
        .bind(event.application_id)
        .bind(event.event_id)
        .bind(digest)
        .bind(&result)
        .bind(json!(event))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(result)
}
pub(crate) async fn quarantine(
    app: &App,
    headers: &HeaderMap,
    id: Uuid,
    event: Uuid,
) -> Result<Value> {
    admin(app, headers)?;
    repository::quarantine()
        .bind(id)
        .bind(event)
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    audit(app, id, "event_quarantined", json!({"event_id":event})).await?;
    Ok(
        json!({"status":"quarantined","notice":"Create a fresh snapshot and model; historical snapshots remain reproducible until deleted."}),
    )
}

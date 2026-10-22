use super::super::{
    audit::audit,
    auth::admin,
    error::{Result, bad, db_error},
    state::App,
};
use crate::server::repositories::models as repository;
use axum::http::HeaderMap;
use pragma_behavior::{Event, History, Model, Outcome, Policy, evaluate};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
pub(crate) async fn train(app: &App, headers: &HeaderMap, id: Uuid, body: Value) -> Result<Value> {
    admin(app, headers)?;
    let job = body
        .get("idempotency_key")
        .map(|v| Uuid::parse_str(v.as_str().unwrap_or("")))
        .transpose()
        .map_err(bad)?
        .unwrap_or_else(Uuid::new_v4);
    repository::enqueue()
        .bind(job)
        .bind(id)
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    let previous: Uuid = repository::job_snapshot()
        .bind(job)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    if previous != id {
        return Err(bad("training identity belongs to another snapshot"));
    }
    Ok(json!({"id":job}))
}
pub(crate) async fn jobs(app: &App, headers: &HeaderMap) -> Result<Value> {
    admin(app, headers)?;
    repository::jobs()
        .fetch_one(&app.db)
        .await
        .map_err(db_error)
}
pub(crate) async fn models(app: &App, headers: &HeaderMap) -> Result<Value> {
    admin(app, headers)?;
    repository::list()
        .fetch_one(&app.db)
        .await
        .map_err(db_error)
}
pub(crate) async fn activate(app: &App, headers: &HeaderMap, id: Uuid) -> Result<Value> {
    admin(app, headers)?;
    let application: Uuid = repository::application()
        .bind(id)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    let mut tx = app.db.begin().await.map_err(db_error)?;
    repository::lock_application()
        .bind(application.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    repository::activate()
        .bind(application)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    // Sequence surprise history belongs to one model generation; cold-start rather than mix scales.
    repository::clear_history()
        .bind(application)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    audit(app, application, "model_activated", json!({"model_id":id})).await?;
    Ok(json!({"status":"activated"}))
}
pub(crate) async fn replay(app: &App, headers: &HeaderMap, id: Uuid, body: Value) -> Result<Value> {
    admin(app, headers)?;
    let model = load_model(app, Some(id))
        .await?
        .ok_or_else(|| bad("model missing"))?;
    let snapshot = Uuid::parse_str(body["snapshot_id"].as_str().unwrap_or("")).map_err(bad)?;
    let row = repository::snapshot_events()
        .bind(snapshot)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    if row.get::<Uuid, _>("application_id") != model.application_id {
        return Err(bad("model and snapshot scope mismatch"));
    }
    let policy_value: Value = repository::policy()
        .bind(model.application_id)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    let mut policy: Policy = serde_json::from_value(policy_value).map_err(bad)?;
    policy.enforce = false;
    let events: Vec<Event> = serde_json::from_value(row.get("events")).map_err(bad)?;
    let mut histories = std::collections::BTreeMap::<String, History>::new();
    let mut sample = vec![];
    let mut flagged = 0;
    let mut evaluated = 0;
    for event in &events {
        if event.outcome == Outcome::Attempted {
            continue;
        }
        let history = histories.entry(event.partition()).or_default();
        let result = evaluate(Some(&model), &policy, event, history);
        evaluated += 1;
        if result.recommended_action != "allow" {
            flagged += 1;
        }
        if sample.len() < 100 {
            sample.push(json!({"event_id":event.event_id,"action":event.action,"decision":result}));
        }
        history.observe(event);
        if let Some(score) = result.surprise {
            history.surprises.push(score);
            if history.surprises.len() > 128 {
                history.surprises.remove(0);
            }
        }
    }
    Ok(
        json!({"model_id":id,"snapshot_id":snapshot,"evaluated":evaluated,"would_check_or_block":flagged,"sample":sample,"live_state_changed":false,"notice":"Unlabeled replay measures flag rate, not detection accuracy. Compare separate labeled scenarios before enforcement."}),
    )
}
pub(crate) async fn load_model(app: &App, id: Option<Uuid>) -> Result<Option<Model>> {
    let Some(id) = id else {
        return Ok(None);
    };
    let value: Value = repository::load()
        .bind(id)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    Ok(Some(serde_json::from_value(value).map_err(bad)?))
}

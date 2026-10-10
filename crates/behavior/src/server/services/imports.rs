use crate::server::{
    auth::admin,
    contracts::RecordingImport,
    error::{Result, bad, db_error},
    privacy::{hash, sanitize},
    repositories::{audit, collection, imports as repository},
    state::App,
};
use axum::http::HeaderMap;
use pragma_behavior::{Event, Outcome, Policy};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Import a complete recording atomically. Historical uploads never enter live sequence state.
pub(crate) async fn import(
    app: &App,
    headers: &HeaderMap,
    id: Uuid,
    input: RecordingImport,
) -> Result<Value> {
    admin(app, headers)?;
    let manifest = input.manifest;
    manifest.validate_content(&input.content).map_err(bad)?;
    if manifest.config.format != "behavior_events" || manifest.config.application_id != Some(id) {
        return Err(bad(
            "recording must contain behavior events for the selected application",
        ));
    }
    let snapshot = manifest.recording_id;
    // Include the manifest in the identity: changing privacy selections must not silently reuse an import.
    let source_hash = hash(serde_json::to_vec(&manifest).map_err(bad)?);
    let mut tx = app.db.begin().await.map_err(db_error)?;
    collection::lock_application_shared()
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    collection::lock_partition()
        .bind(format!("recording:{snapshot}"))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    if let Some(old) = repository::identity()
        .bind(snapshot)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
    {
        if old.get::<Uuid, _>("application_id") != id
            || old.get::<Option<String>, _>("source_hash").as_deref() != Some(&source_hash)
        {
            return Err(bad(
                "recording ID reused with different content or settings",
            ));
        }
        return Ok(
            json!({"id":snapshot,"event_count":old.get::<i32,_>("count"),"content_hash":old.get::<String,_>("content_hash"),"live_state_changed":false}),
        );
    }
    let row = repository::policy()
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or_else(|| bad("application not found"))?;
    let policy: Policy = serde_json::from_value(row.get("policy")).map_err(bad)?;
    let mut events = Vec::with_capacity(manifest.row_count);
    let mut ids = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    for (line, text) in input.content.lines().enumerate() {
        if text.len() > 16 * 1024 {
            return Err(bad(format!("row {} exceeds 16 KiB", line + 1)));
        }
        let value = serde_json::from_str(text)
            .map_err(|_| bad(format!("row {} is not valid JSON", line + 1)))?;
        let filtered = manifest.config.sanitize(value).map_err(|_| {
            bad(format!(
                "row {} does not match the event contract or recording scope",
                line + 1
            ))
        })?;
        let event: Event = serde_json::from_value(filtered).map_err(bad)?;
        if !ids.insert(event.event_id) {
            return Err(bad(format!("duplicate event ID at row {}", line + 1)));
        }
        if event.outcome != Outcome::Attempted
            && let Some(correlation) = event.correlation_id
            && !outcomes.insert(correlation)
        {
            return Err(bad(
                "multiple confirmed outcomes refer to the same attempted action",
            ));
        }
        events.push(sanitize(app, &policy, event));
    }
    let attempts: BTreeMap<_, _> = events
        .iter()
        .filter(|e| e.outcome == Outcome::Attempted)
        .map(|e| (e.event_id, e))
        .collect();
    for event in &events {
        if event.outcome != Outcome::Attempted
            && let Some(attempt) = event.correlation_id.and_then(|id| attempts.get(&id))
            && (attempt.partition() != event.partition()
                || attempt.action != event.action
                || attempt.resource != event.resource)
        {
            return Err(bad("outcome does not match the attempt in this recording"));
        }
    }
    events.sort_by_key(|event| {
        (
            event.timestamp_ms,
            event.sequence_number.unwrap_or(u64::MAX),
            event.event_id,
        )
    });
    let digest = hash(serde_json::to_vec(&events).map_err(bad)?);
    repository::insert()
        .bind(snapshot)
        .bind(id)
        .bind(&digest)
        .bind(json!(events))
        .bind(source_hash)
        .bind(json!(manifest))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    audit::append()
        .bind(id)
        .bind("recording_imported")
        .bind(json!({"snapshot_id":snapshot,"event_count":events.len(),"content_hash":digest}))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    tx.commit().await.map_err(db_error)?;
    Ok(
        json!({"id":snapshot,"event_count":events.len(),"content_hash":digest,"live_state_changed":false}),
    )
}

use super::super::{
    audit::audit,
    auth::admin,
    error::{Result, bad, db_error},
    privacy::hash,
    state::App,
};
use crate::server::repositories::snapshots as repository;
use axum::http::HeaderMap;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
pub(crate) async fn snapshot(
    app: &App,
    headers: &HeaderMap,
    id: Uuid,
    body: Value,
) -> Result<Value> {
    admin(app, headers)?;
    let snapshot = body
        .get("idempotency_key")
        .map(|v| Uuid::parse_str(v.as_str().unwrap_or("")))
        .transpose()
        .map_err(bad)?
        .unwrap_or_else(Uuid::new_v4);
    if let Some(row) = repository::identity()
        .bind(snapshot)
        .fetch_optional(&app.db)
        .await
        .map_err(db_error)?
    {
        if row.get::<Uuid, _>("application_id") != id {
            return Err(bad("snapshot identity belongs to another application"));
        }
        return Ok(
            json!({"id":snapshot,"content_hash":row.get::<String,_>("content_hash"),"event_count":row.get::<i32,_>("count")}),
        );
    }
    let events: Vec<Value> = repository::retained_events()
        .bind(id)
        .fetch_all(&app.db)
        .await
        .map_err(db_error)?;
    if events.is_empty() || events.len() > 100000 {
        return Err(bad("snapshot requires 1–100,000 retained events"));
    }
    let digest = hash(serde_json::to_vec(&events).map_err(bad)?);
    repository::insert()
        .bind(snapshot)
        .bind(id)
        .bind(&digest)
        .bind(json!(events))
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    let stored = repository::identity()
        .bind(snapshot)
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    if stored.get::<Uuid, _>("application_id") != id {
        return Err(bad("snapshot identity belongs to another application"));
    }
    let digest: String = stored.get("content_hash");
    audit(
        app,
        id,
        "snapshot_created",
        json!({"snapshot_id":snapshot,"content_hash":digest}),
    )
    .await?;
    Ok(json!({"id":snapshot,"content_hash":digest,"event_count":stored.get::<i32,_>("count")}))
}
pub(crate) async fn snapshots(app: &App, headers: &HeaderMap) -> Result<Value> {
    admin(app, headers)?;
    repository::list()
        .fetch_one(&app.db)
        .await
        .map_err(db_error)
}

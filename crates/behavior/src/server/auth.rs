use super::{
    error::{Result, bad, db_error},
    privacy::hash,
    state::App,
};
use crate::server::repositories::auth as repository;
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use pragma_behavior::Policy;
use serde_json::json;
use sqlx::Row;
use subtle::ConstantTimeEq;
use uuid::Uuid;
pub(super) fn token(headers: &HeaderMap) -> &str {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}
pub(super) fn admin(app: &App, headers: &HeaderMap) -> Result<()> {
    if bool::from(token(headers).as_bytes().ct_eq(app.admin.as_bytes())) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"administrator authentication required"})),
        ))
    }
}
pub(super) async fn authenticate(
    app: &App,
    headers: &HeaderMap,
    id: Uuid,
) -> Result<(Policy, Option<Uuid>)> {
    let row = repository::credential()
        .bind(id)
        .bind(hash(token(headers)))
        .fetch_optional(&app.db)
        .await
        .map_err(db_error)?;
    let Some(row) = row else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid application credential"})),
        ));
    };
    Ok((
        serde_json::from_value(row.get("policy")).map_err(bad)?,
        row.get("active_model"),
    ))
}

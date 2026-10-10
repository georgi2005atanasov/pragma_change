use super::super::{error::Result, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) async fn train(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    super::super::services::models::train(&app, &headers, id, body)
        .await
        .map(Json)
}
pub(crate) async fn jobs(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>> {
    super::super::services::models::jobs(&app, &headers)
        .await
        .map(Json)
}
pub(crate) async fn models(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>> {
    super::super::services::models::models(&app, &headers)
        .await
        .map(Json)
}
pub(crate) async fn activate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    super::super::services::models::activate(&app, &headers, id)
        .await
        .map(Json)
}
pub(crate) async fn replay(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    super::super::services::models::replay(&app, &headers, id, body)
        .await
        .map(Json)
}

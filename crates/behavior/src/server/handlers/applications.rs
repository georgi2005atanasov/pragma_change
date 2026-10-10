use super::super::{contracts::NewApp, error::Result, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use pragma_behavior::Policy;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) async fn create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewApp>,
) -> Result<Json<Value>> {
    super::super::services::applications::create(&app, &headers, input)
        .await
        .map(Json)
}
pub(crate) async fn list(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>> {
    super::super::services::applications::list(&app, &headers)
        .await
        .map(Json)
}
pub(crate) async fn policy(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Policy>,
) -> Result<Json<Value>> {
    super::super::services::applications::policy(&app, &headers, id, input)
        .await
        .map(Json)
}
pub(crate) async fn purge(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    super::super::services::applications::purge(&app, &headers, id)
        .await
        .map(Json)
}
pub(crate) async fn health(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    super::super::services::applications::health(&app, &headers, id)
        .await
        .map(Json)
}

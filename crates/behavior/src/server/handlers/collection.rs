use super::super::{contracts::Batch, error::Result, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use pragma_behavior::Event;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) async fn record(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Batch>,
) -> Result<Json<Value>> {
    super::super::services::collection::record(&app, &headers, input)
        .await
        .map(Json)
}
pub(crate) async fn check(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(event): Json<Event>,
) -> Result<Json<Value>> {
    super::super::services::collection::check(&app, &headers, event)
        .await
        .map(Json)
}
pub(crate) async fn quarantine(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path((id, event)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    super::super::services::collection::quarantine(&app, &headers, id, event)
        .await
        .map(Json)
}

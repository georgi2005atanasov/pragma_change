use super::super::{error::Result, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) async fn snapshot(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>> {
    super::super::services::snapshots::snapshot(&app, &headers, id, body)
        .await
        .map(Json)
}
pub(crate) async fn snapshots(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    super::super::services::snapshots::snapshots(&app, &headers)
        .await
        .map(Json)
}

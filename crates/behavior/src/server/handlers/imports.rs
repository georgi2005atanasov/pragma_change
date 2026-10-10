use crate::server::{contracts::RecordingImport, error::Result, services, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) async fn import(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<RecordingImport>,
) -> Result<Json<Value>> {
    services::imports::import(&app, &headers, id, input)
        .await
        .map(Json)
}

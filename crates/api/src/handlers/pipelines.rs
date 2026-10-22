use crate::services::pipelines::Replay;
use crate::{error::Result, repositories, services, state::State};
use axum::{
    Json,
    extract::{Path, State as AxumState},
};
use pragma_domain::Pipeline;
use serde_json::Value;
use uuid::Uuid;
pub(crate) async fn create_pipeline(
    AxumState(app): AxumState<State>,
    Json(p): Json<Pipeline>,
) -> Result<Json<Value>> {
    services::pipelines::create_pipeline(app, p).await.map(Json)
}
pub(crate) async fn template(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    services::pipelines::template(app, id).await.map(Json)
}

pub(crate) async fn replay(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
    Json(input): Json<Replay>,
) -> Result<Json<Value>> {
    services::pipelines::replay(app, id, input).await.map(Json)
}

pub(crate) async fn list_pipelines(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::pipelines::list(&app.db).await.map(Json)
}

use crate::services::training::Train;
use crate::{error::Result, repositories, services, state::State};
use axum::{
    Json,
    extract::{Path, State as AxumState},
};
use serde_json::Value;
use uuid::Uuid;
pub(crate) async fn train(
    AxumState(app): AxumState<State>,
    Json(input): Json<Train>,
) -> Result<Json<Value>> {
    services::training::train(app, input).await.map(Json)
}
pub(crate) async fn retry(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    services::training::retry(app, id).await.map(Json)
}

pub(crate) async fn list_jobs(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::training_jobs::list(&app.db).await.map(Json)
}

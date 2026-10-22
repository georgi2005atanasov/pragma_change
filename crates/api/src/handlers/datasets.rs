use crate::data;
use crate::{error::Result, repositories, services, state::State};
use axum::{Json, extract::State as AxumState};
use serde_json::Value;
pub(crate) async fn preview_dataset(
    AxumState(app): AxumState<State>,
    Json(input): Json<data::Import>,
) -> Result<Json<Value>> {
    services::datasets::preview_dataset(app, input)
        .await
        .map(Json)
}
pub(crate) async fn import_dataset(
    AxumState(app): AxumState<State>,
    Json(input): Json<data::Import>,
) -> Result<Json<Value>> {
    services::datasets::import_dataset(app, input)
        .await
        .map(Json)
}

pub(crate) async fn list_datasets(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::datasets::list(&app.db).await.map(Json)
}

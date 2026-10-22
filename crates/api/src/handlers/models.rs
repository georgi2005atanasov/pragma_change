use crate::services::models::Score;
use crate::{error::Result, repositories, services, state::State};
use axum::{
    Json,
    extract::{Path, State as AxumState},
};
use serde_json::Value;
use uuid::Uuid;
pub(crate) async fn score_model(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
    Json(input): Json<Score>,
) -> Result<Json<Value>> {
    services::models::score_model(app, id, input)
        .await
        .map(Json)
}

pub(crate) async fn list_models(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::models::list(&app.db).await.map(Json)
}

pub(crate) async fn model_detail(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    repositories::models::detail(&app.db, id).await.map(Json)
}

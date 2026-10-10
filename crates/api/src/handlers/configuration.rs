use crate::services::configuration::{NewApi, NewEndpoint};
use crate::{error::Result, repositories, services, state::State};
use axum::{Json, extract::State as AxumState};
use serde_json::Value;
pub(crate) async fn create_api(
    AxumState(app): AxumState<State>,
    Json(input): Json<NewApi>,
) -> Result<Json<Value>> {
    services::configuration::create_api(app, input)
        .await
        .map(Json)
}

pub(crate) async fn create_endpoint(
    AxumState(app): AxumState<State>,
    Json(input): Json<NewEndpoint>,
) -> Result<Json<Value>> {
    services::configuration::create_endpoint(app, input)
        .await
        .map(Json)
}

pub(crate) async fn list_apis(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::configuration::list_apis(&app.db)
        .await
        .map(Json)
}

pub(crate) async fn list_endpoints(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::configuration::list_endpoints(&app.db)
        .await
        .map(Json)
}

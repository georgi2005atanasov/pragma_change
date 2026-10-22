use crate::{error::Result, repositories, state::State};
use axum::{Json, extract::State as AxumState};
use serde_json::Value;

pub(crate) async fn list_audits(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::reporting::list_audits(&app.db)
        .await
        .map(Json)
}

pub(crate) async fn overview(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::reporting::overview(&app.db).await.map(Json)
}

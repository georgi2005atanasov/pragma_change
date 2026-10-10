use crate::services::releases::NewRelease;
use crate::{error::Result, repositories, services, state::State};
use axum::{
    Json,
    extract::{Path, State as AxumState},
};
use serde_json::Value;
use uuid::Uuid;
pub(crate) async fn create_release(
    AxumState(app): AxumState<State>,
    Json(input): Json<NewRelease>,
) -> Result<Json<Value>> {
    services::releases::create_release(app, input)
        .await
        .map(Json)
}

pub(crate) async fn list_releases(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    repositories::releases::list(&app.db).await.map(Json)
}

pub(crate) async fn download_release(
    AxumState(app): AxumState<State>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response> {
    use axum::{http::header, response::IntoResponse};
    let bytes = services::releases::download(&app, id).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=pragmachange-{id}.tar.gz"),
            ),
        ],
        bytes,
    )
        .into_response())
}

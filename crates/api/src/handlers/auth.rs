use crate::{error::Result, services::auth, state::State};
use axum::{
    Json,
    extract::{Request, State as AxumState},
    http::{HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use serde_json::Value;

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

pub(crate) async fn login(
    AxumState(app): AxumState<State>,
    Json(input): Json<auth::Login>,
) -> Result<Json<Value>> {
    auth::login(&app, input).await.map(Json)
}

pub(crate) async fn authorize(
    AxumState(app): AxumState<State>,
    request: Request,
    next: Next,
) -> Result<Response> {
    let csrf = if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        None
    } else {
        Some(
            request
                .headers()
                .get("x-csrf-token")
                .and_then(|header| header.to_str().ok())
                .unwrap_or(""),
        )
    };
    auth::authorize(&app, bearer(request.headers()), csrf).await?;
    Ok(next.run(request).await)
}

pub(crate) async fn logout(
    AxumState(app): AxumState<State>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    auth::logout(&app, bearer(&headers)).await.map(Json)
}

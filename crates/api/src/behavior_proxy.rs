use crate::{
    error::{Error, Result, bad},
    state::{App, State},
};
use axum::{
    Json,
    extract::{Path, State as AxumState},
    http::{Method, StatusCode},
};
use serde_json::Value;
pub(crate) async fn request(
    app: &App,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    if path.len() > 200
        || path
            .split('/')
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return Err(bad("Invalid behavior API path"));
    }
    let url = std::env::var("BEHAVIOR_URL").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Configure BEHAVIOR_URL to connect the behavior service".into(),
        )
    })?;
    let token = std::env::var("BEHAVIOR_ADMIN_TOKEN")
        .map_err(|_| bad("Configure the behavior service administrator token"))?;
    let mut request = app
        .http
        .request(
            method,
            format!("{}/admin/{path}", url.trim_end_matches('/')),
        )
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(15));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Behavior service unavailable".into(),
        )
    })?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| bad("Invalid behavior service response"))?;
    if !status.is_success() {
        return Err(Error(
            status,
            value["error"]
                .as_str()
                .unwrap_or("Behavior operation failed")
                .into(),
        ));
    }
    Ok(value)
}
pub(crate) async fn get(
    AxumState(app): AxumState<State>,
    Path(path): Path<String>,
) -> Result<Json<Value>> {
    request(&app, Method::GET, &path, None).await.map(Json)
}
pub(crate) async fn post(
    AxumState(app): AxumState<State>,
    Path(path): Path<String>,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    request(&app, Method::POST, &path, Some(value))
        .await
        .map(Json)
}

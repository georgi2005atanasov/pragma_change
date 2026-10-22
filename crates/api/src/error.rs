use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub(crate) struct Error(pub(crate) StatusCode, pub(crate) String);
pub(crate) type Result<T> = std::result::Result<T, Error>;
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error=%e,"database operation failed");
        match &e {
            sqlx::Error::RowNotFound => Self(StatusCode::NOT_FOUND, "not found".into()),
            sqlx::Error::Database(d) if d.is_unique_violation() || d.is_foreign_key_violation() => {
                Self(
                    StatusCode::CONFLICT,
                    "duplicate or referenced resource".into(),
                )
            }
            _ => Self(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database operation failed".into(),
            ),
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        tracing::error!(error=%e,"artifact operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "artifact operation failed".into(),
        )
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        bad(e.to_string())
    }
}
impl From<pragma_domain::ValidationError> for Error {
    fn from(e: pragma_domain::ValidationError) -> Self {
        bad(e.to_string())
    }
}
pub(crate) fn bad(s: impl Into<String>) -> Error {
    Error(StatusCode::UNPROCESSABLE_ENTITY, s.into())
}

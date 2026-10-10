use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
pub(super) type Error = (StatusCode, Json<Value>);
pub(super) type Result<T> = std::result::Result<T, Error>;
pub(super) fn bad(message: impl ToString) -> Error {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({"error":message.to_string()})),
    )
}
pub(super) fn db_error(_: sqlx::Error) -> Error {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"behavior storage operation failed"})),
    )
}

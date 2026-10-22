use super::{handlers::*, state::App};
use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::json;
use std::sync::Arc;
pub(super) fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/v1/events", post(record))
        .route("/v1/evaluate", post(check))
        .route("/admin/apps", get(list).post(create))
        .route("/admin/apps/{id}/policy", post(policy))
        .route("/admin/apps/{id}/snapshots", post(snapshot))
        .route("/admin/snapshots", get(snapshots))
        .route("/admin/snapshots/{id}/train", post(train))
        .route("/admin/jobs", get(jobs))
        .route("/admin/models", get(models))
        .route("/admin/models/{id}/activate", post(activate))
        .route("/admin/models/{id}/replay", post(replay))
        .route("/admin/apps/{id}/purge", post(purge))
        .route(
            "/admin/apps/{id}/events/{event}/quarantine",
            post(quarantine),
        )
        .route("/admin/apps/{id}/health", get(health))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .route(
            "/admin/apps/{id}/imports",
            post(import).layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .with_state(app)
}

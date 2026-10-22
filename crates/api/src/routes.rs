use crate::{
    handlers::{auth, configuration, datasets, models, pipelines, releases, reporting, training},
    state::State,
};
use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::Value;

pub fn router() -> Router<State> {
    Router::new()
        .route("/api/v1/config-analysis", post(crate::remote::analyze))
        .route(
            "/api/v1/behavior/{*path}",
            get(crate::behavior_proxy::get).post(crate::behavior_proxy::post),
        )
        .route(
            "/api/v1/onboarding/provider",
            get(crate::onboarding::provider).post(crate::onboarding::configure),
        )
        .route(
            "/api/v1/onboarding/jobs",
            get(crate::onboarding::list).post(crate::onboarding::start),
        )
        .route(
            "/api/v1/onboarding/jobs/{id}/apply",
            post(crate::onboarding::apply),
        )
        .route(
            "/api/v1/servers",
            get(crate::remote::list).post(crate::remote::create),
        )
        .route("/api/v1/servers/{id}/jobs", post(crate::remote::enqueue))
        .route("/api/v1/remote-jobs", get(crate::remote::jobs))
        .route("/api/v1/remote-rollouts", post(crate::remote::rollout))
        .route("/api/v1/logout", post(auth::logout))
        .route("/api/v1/overview", get(reporting::overview))
        .route(
            "/api/v1/apis",
            get(configuration::list_apis).post(configuration::create_api),
        )
        .route(
            "/api/v1/endpoints",
            get(configuration::list_endpoints).post(configuration::create_endpoint),
        )
        .route(
            "/api/v1/pipelines",
            get(pipelines::list_pipelines).post(pipelines::create_pipeline),
        )
        .route("/api/v1/pipelines/{id}/template", get(pipelines::template))
        .route("/api/v1/pipelines/{id}/replay", post(pipelines::replay))
        .route(
            "/api/v1/datasets",
            get(datasets::list_datasets).post(datasets::import_dataset),
        )
        .route("/api/v1/datasets/preview", post(datasets::preview_dataset))
        .route(
            "/api/v1/training-jobs",
            get(training::list_jobs).post(training::train),
        )
        .route("/api/v1/training-jobs/{id}/retry", post(training::retry))
        .route("/api/v1/models", get(models::list_models))
        .route("/api/v1/models/{id}", get(models::model_detail))
        .route("/api/v1/models/{id}/score", post(models::score_model))
        .route(
            "/api/v1/releases",
            get(releases::list_releases).post(releases::create_release),
        )
        .route(
            "/api/v1/releases/{id}/download",
            get(releases::download_release),
        )
        .route("/api/v1/audit-events", get(reporting::list_audits))
        .route("/api/v1/openapi.json", get(openapi))
}
async fn openapi() -> Json<Value> {
    Json(
        serde_json::from_str(include_str!("../../../docs/openapi.json"))
            .expect("checked-in OpenAPI"),
    )
}

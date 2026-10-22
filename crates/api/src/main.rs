mod artifacts;
mod behavior_proxy;
mod data;
mod error;
mod handlers;
mod hash;
mod onboarding;
mod remote;
mod repositories;
mod routes;
mod secrets;
mod services;
mod state;
#[cfg(test)]
mod tests;
mod worker;

use axum::{Json, Router};
use serde_json::json;
use state::App;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pragma_api=info".into()),
        )
        .init();
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    sqlx::migrate!().run(&db).await?;
    services::auth::bootstrap(&db).await?;
    let secret: [u8; 32] = hex::decode(std::env::var("PARTITION_SECRET")?)?
        .try_into()
        .map_err(|_| "PARTITION_SECRET must be 64 hex characters")?;
    let artifacts = PathBuf::from(std::env::var("ARTIFACT_DIR").unwrap_or("artifacts".into()));
    tokio::fs::create_dir_all(&artifacts).await?;
    let artifacts = tokio::fs::canonicalize(artifacts).await?;
    tracing::info!(path=%artifacts.display(), "Artifact storage ready; retain this directory together with the database");
    let state = Arc::new(App {
        db,
        artifacts,
        trainer_url: std::env::var("TRAINER_URL").unwrap_or("http://127.0.0.1:8001".into()),
        trainer_token: std::env::var("TRAINER_TOKEN")?,
        secret,
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
    });
    tokio::spawn(worker::run(state.clone()));
    tokio::spawn(remote::run(state.clone()));
    tokio::spawn(onboarding::run(state.clone()));
    let protected = routes::router().route_layer(axum::middleware::from_fn_with_state(
        state.clone(),
        handlers::auth::authorize,
    ));
    let app = Router::new()
        .route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/api/v1/login", axum::routing::post(handlers::auth::login))
        .merge(protected)
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(
        std::env::var("BIND_ADDR").unwrap_or("127.0.0.1:8080".into()),
    )
    .await?;
    tracing::info!(address=%listener.local_addr()?,"PragmaChange management API started");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

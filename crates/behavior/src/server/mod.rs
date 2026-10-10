mod audit;
mod auth;
mod contracts;
mod error;
mod handlers;
mod privacy;
mod repositories;
mod routes;
mod services;
mod state;
mod worker;
use state::App;
use std::sync::Arc;
use worker::worker_tick;
pub(crate) async fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("BEHAVIOR_DATABASE_URL")?)
        .await?;
    sqlx::migrate!().run(&db).await?;
    let admin_token = std::env::var("BEHAVIOR_ADMIN_TOKEN")?;
    if admin_token.len() < 32 {
        return Err("BEHAVIOR_ADMIN_TOKEN must be at least 32 characters".into());
    }
    let secret = hex::decode(std::env::var("BEHAVIOR_PARTITION_SECRET")?)?;
    if secret.len() != 32 {
        return Err("32-byte behavior partition secret required".into());
    }
    let app = Arc::new(App {
        db,
        admin: admin_token,
        secret,
    });
    let worker = app.clone();
    tokio::spawn(async move {
        loop {
            if worker_tick(&worker).await.is_err() {
                eprintln!("behavior worker operation failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
    let router = routes::router(app);
    let listener = tokio::net::TcpListener::bind(
        std::env::var("BEHAVIOR_BIND_ADDR").unwrap_or("127.0.0.1:8090".into()),
    )
    .await?;
    axum::serve(listener, router).await?;
    Ok(())
}
#[cfg(test)]
mod tests;

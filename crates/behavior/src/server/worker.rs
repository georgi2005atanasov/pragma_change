use super::state::App;
use crate::server::repositories::worker as repository;
use pragma_behavior::{Event, Model};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
pub(super) async fn worker_tick(
    app: &App,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    repository::expire_leases().execute(&app.db).await?;
    let job = repository::claim().fetch_optional(&app.db).await?;
    if let Some(job) = job {
        let id: Uuid = job.get("id");
        let snapshot: Uuid = job.get("snapshot_id");
        let value: Value = repository::events()
            .bind(snapshot)
            .fetch_one(&app.db)
            .await?;
        let events: Vec<Event> = serde_json::from_value(value)?;
        match tokio::task::spawn_blocking(move || Model::train(&events)).await? {
            Ok(mut model) => {
                model.id = id;
                let mut tx = app.db.begin().await?;
                let rows = repository::complete()
                    .bind(id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if rows == 1 {
                    repository::save_model()
                        .bind(id)
                        .bind(model.application_id)
                        .bind(snapshot)
                        .bind(json!(model))
                        .execute(&mut *tx)
                        .await?;
                }
                tx.commit().await?;
            }
            Err(error) => {
                repository::fail()
                    .bind(id)
                    .bind(error)
                    .execute(&app.db)
                    .await?;
            }
        }
    }
    // Expire raw data and live state; snapshots/models use explicit deletion to preserve provenance.
    for table in ["behavior_events", "behavior_decisions", "behavior_state"] {
        repository::expire_data(table).execute(&app.db).await?;
    }
    Ok(())
}

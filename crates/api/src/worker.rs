use crate::{
    error::Result,
    repositories::training_jobs,
    services::training,
    state::{App, State},
};

pub async fn tick(app: &App) -> Result<bool> {
    let job = training_jobs::claim(&app.db).await?;
    // A final attempt interrupted by a process crash must eventually become failed.
    training_jobs::expire_final_attempts(&app.db).await?;
    let Some(job) = job else {
        return Ok(false);
    };
    let id = job.id;
    let attempt = job.attempts;
    if let Err(error) = training::execute(app, id, job.dataset_id, job.parameters, attempt).await {
        tracing::warn!(%id,error=%error.1,"training attempt failed");
        training_jobs::record_failure(&app.db, id, attempt, &error.1, error.0.is_client_error())
            .await?;
    }
    Ok(true)
}
pub async fn run(app: State) {
    loop {
        match tick(&app).await {
            Ok(true) => continue,
            Err(e) => tracing::error!(error=%e.1,"training worker error"),
            _ => (),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

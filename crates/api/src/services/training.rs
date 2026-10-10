use crate::{
    artifacts::write as artifact,
    error::{Result, bad},
    hash::hash,
    repositories::{self, reporting::audit},
    state::{App, State},
};
use pragma_domain::Forest;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Train {
    dataset_id: Uuid,
    #[serde(default)]
    parameters: Parameters,
}

pub(crate) async fn train(app: State, input: Train) -> Result<Value> {
    input.parameters.validate()?;
    dataset_vectors(&app, input.dataset_id).await?;
    let id = Uuid::new_v4();
    repositories::training_jobs::insert(
        &app.db,
        id,
        input.dataset_id,
        serde_json::to_value(input.parameters)?,
    )
    .await?;
    audit(
        &app.db,
        "training_queued",
        id,
        json!({"dataset_id":input.dataset_id}),
    )
    .await?;
    Ok(json!({"id":id,"status":"queued"}))
}

pub(crate) async fn retry(app: State, id: Uuid) -> Result<Value> {
    let dataset_id = repositories::training_jobs::dataset(&app.db, id).await?;
    dataset_vectors(&app, dataset_id).await?;
    if !repositories::training_jobs::retry(&app.db, id).await? {
        return Err(bad("only failed jobs can be retried"));
    }
    audit(&app.db, "training_retried", id, json!({})).await?;
    Ok(json!({"id":id,"status":"queued"}))
}

#[derive(serde::Serialize, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Parameters {
    pub n_estimators: u32,
    pub max_samples: u32,
    pub seed: u32,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            n_estimators: 100,
            max_samples: 256,
            seed: 42,
        }
    }
}
impl Parameters {
    pub fn validate(&self) -> Result<()> {
        if !(1..=512).contains(&self.n_estimators) || !(2..=65536).contains(&self.max_samples) {
            return Err(bad(
                "n_estimators must be 1–512; max_samples must be 2–65536",
            ));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct TrainingResponse {
    model: Forest,
    evaluation: Value,
    parity_vectors: Vec<Vec<f64>>,
    parity_scores: Vec<f64>,
}
pub(crate) async fn execute(
    app: &App,
    id: Uuid,
    dataset: Uuid,
    parameters: Value,
    attempt: i32,
) -> Result<()> {
    let dataset_id = dataset;
    let dataset = repositories::datasets::artifact(&app.db, dataset_id).await?;
    let pipeline = repositories::pipelines::load(&app.db, dataset.pipeline_version_id).await?;
    let bytes = crate::artifacts::read(
        app,
        &dataset.artifact_path,
        &dataset.content_hash,
        &format!("Dataset {}", dataset_id),
    )
    .await?;
    let vectors: Vec<Vec<f64>> = serde_json::from_slice(&bytes)?;
    let response = app.http
        .post(format!("{}/v1/train", app.trainer_url.trim_end_matches('/')))
        .bearer_auth(&app.trainer_token)
        .json(&json!({
            "job_id": id,
            "pipeline_version_id": pipeline.id,
            "schema_hash": pipeline.schema_hash(),
            "feature_names": pipeline.features.iter().map(|feature| &feature.name).collect::<Vec<_>>(),
            "vectors": vectors,
            "parameters": parameters
        }))
        .send().await.map_err(|error| crate::error::Error(axum::http::StatusCode::BAD_GATEWAY, format!("Trainer unavailable: {error}")))?;
    if !response.status().is_success() {
        let status = response.status();
        if status.is_client_error() && status.as_u16() != 408 && status.as_u16() != 429 {
            return Err(bad(format!(
                "Trainer rejected the request ({status}). Check dataset and trainer configuration before retrying."
            )));
        }
        return Err(crate::error::Error(
            axum::http::StatusCode::BAD_GATEWAY,
            format!("trainer returned {}", response.status()),
        ));
    }
    let data: TrainingResponse = response
        .json()
        .await
        .map_err(|e| bad(format!("invalid trainer response: {e}")))?;
    data.model.validate(&pipeline)?;
    if data.model.id.0 != id
        || data.parity_vectors.is_empty()
        || data.parity_vectors.len() != data.parity_scores.len()
    {
        return Err(bad("invalid model identity or parity report"));
    }
    for (vector, expected) in data.parity_vectors.iter().zip(&data.parity_scores) {
        let actual = pragma_engine::score(&data.model, vector).map_err(|e| bad(e.to_string()))?;
        if !expected.is_finite() || (actual - expected).abs() > 1e-7 {
            return Err(bad("Python/Rust scoring parity failed"));
        }
    }
    let bytes = serde_json::to_vec(&data.model)?;
    let name = artifact(app, &format!("{id}.model.json"), &bytes).await?;
    let mut transaction = app.db.begin().await?;
    // Fencing prevents an expired lease from committing after another worker reclaimed it.
    if !repositories::training_jobs::complete(&mut transaction, id, attempt).await? {
        return Err(bad("training lease superseded"));
    }
    repositories::models::insert(
        &mut transaction,
        repositories::models::NewModel {
            id,
            pipeline_version_id: pipeline.id.0,
            schema_hash: &pipeline.schema_hash(),
            artifact_path: &name,
            content_hash: &hash(bytes),
            evaluation: &data.evaluation,
        },
    )
    .await?;
    transaction.commit().await?;
    audit(&app.db, "training_succeeded", id, json!({})).await?;
    Ok(())
}

pub(crate) async fn dataset_vectors(app: &App, id: Uuid) -> Result<Vec<Vec<f64>>> {
    let dataset = repositories::datasets::artifact(&app.db, id).await?;
    let bytes = crate::artifacts::read(
        app,
        &dataset.artifact_path,
        &dataset.content_hash,
        &format!("Dataset {id}"),
    )
    .await?;
    Ok(serde_json::from_slice(&bytes)?)
}

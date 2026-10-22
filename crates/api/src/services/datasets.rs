use crate::data;
use crate::{
    artifacts::write as artifact,
    error::{Error, Result, bad},
    hash::hash,
    repositories::{self, reporting::audit},
    state::State,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) async fn preview_dataset(app: State, input: data::Import) -> Result<Value> {
    let pipeline = repositories::pipelines::load(&app.db, input.pipeline_version_id).await?;
    let secret = app.secret;
    tokio::task::spawn_blocking(move || {
        let prepared = data::prepare(&pipeline, &input, &secret)?;
        Ok(data::report(&pipeline, &prepared))
    })
    .await
    .map_err(|e| bad(e.to_string()))?
}

pub(crate) async fn import_dataset(app: State, input: data::Import) -> Result<Value> {
    let pipeline = repositories::pipelines::load(&app.db, input.pipeline_version_id).await?;
    let secret = app.secret;
    let (input, prepared, report) = tokio::task::spawn_blocking(move || {
        let prepared = data::prepare(&pipeline, &input, &secret)?;
        let report = data::report(&pipeline, &prepared);
        Ok::<_, Error>((input, prepared, report))
    })
    .await
    .map_err(|e| bad(e.to_string()))??;
    if !prepared.errors.is_empty() || prepared.vectors.len() < 20 {
        return Err(bad(format!("dataset rejected: {report}")));
    }
    let id = Uuid::new_v4();
    let bytes = serde_json::to_vec(&prepared.vectors)?;
    let path = artifact(&app, &format!("{id}.vectors.json"), &bytes).await?;
    artifact(&app, &format!("{id}.source"), input.content.as_bytes()).await?;
    if let Some(manifest) = &input.recording_manifest {
        artifact(
            &app,
            &format!("{id}.recording.json"),
            &serde_json::to_vec(manifest)?,
        )
        .await?;
    }
    repositories::datasets::insert(
        &app.db,
        repositories::datasets::NewDataset {
            id,
            pipeline_version_id: input.pipeline_version_id,
            name: &input.name,
            format: &input.format,
            schema_hash: &input.schema_hash,
            content_hash: &hash(&bytes),
            artifact_path: &path,
            row_count: prepared.vectors.len() as i32,
            validation_report: &report,
        },
    )
    .await?;
    audit(
        &app.db,
        "dataset_imported",
        id,
        json!({"rows":prepared.vectors.len()}),
    )
    .await?;
    Ok(json!({"id":id,"report":report}))
}

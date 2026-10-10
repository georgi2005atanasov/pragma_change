use crate::{
    error::{Result, bad},
    repositories,
    state::{App, State},
};
use pragma_domain::{Forest, Thresholds};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Score {
    vector: Vec<f64>,
    thresholds: Thresholds,
}

pub(crate) async fn load_model(app: &App, id: Uuid) -> Result<Forest> {
    let model = repositories::models::artifact(&app.db, id).await?;
    let bytes = crate::artifacts::read(
        app,
        &model.artifact_path,
        &model.content_hash,
        &format!("Model {id}"),
    )
    .await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) async fn score_model(app: State, id: Uuid, input: Score) -> Result<Value> {
    let model = load_model(&app, id).await?;
    let pipeline = repositories::pipelines::load(&app.db, model.pipeline_version_id.0).await?;
    model.validate(&pipeline)?;
    input.thresholds.validate()?;
    let features = pragma_engine::FeatureResult {
        partition_keys: BTreeMap::new(),
        feature_names: model.feature_names.clone(),
        vector: input.vector,
        ready: true,
        reason: "ready".into(),
        window_counts: BTreeMap::new(),
    };
    Ok(serde_json::to_value(
        pragma_engine::decide(&model, input.thresholds, &features)
            .map_err(|e| bad(e.to_string()))?,
    )?)
}

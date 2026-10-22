use crate::{
    error::{Result, bad},
    repositories::{self, reporting::audit},
    services::models::load_model,
    state::State,
};
use pragma_domain::{Decision, Pipeline, PipelineId, RequestEvent, Source, Thresholds};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Replay {
    events: Vec<RequestEvent>,
    model_version_id: Option<Uuid>,
    thresholds: Option<Thresholds>,
}

pub(crate) async fn create_pipeline(app: State, mut pipeline: Pipeline) -> Result<Value> {
    pipeline.id = PipelineId(Uuid::new_v4());
    if !pipeline.additional_endpoints.is_empty() {
        return Err(bad(
            "New pipelines require one endpoint. Cross-endpoint model sharing is not supported for new versions; choose one endpoint and train on its traffic. Existing saved versions remain readable.",
        ));
    }
    let mut transaction = app.db.begin().await?;
    let endpoint =
        repositories::configuration::lock_endpoint(&mut transaction, pipeline.endpoint_id).await?;
    pipeline.api_host = endpoint.host;
    pipeline.method = endpoint.method;
    pipeline.path_template = endpoint.path_template;
    pipeline.name = pipeline.name.trim().to_owned();
    pipeline.validate()?;
    let version =
        repositories::pipelines::next_version(&mut transaction, pipeline.endpoint_id).await?;
    repositories::pipelines::insert(&mut transaction, &pipeline, version).await?;
    transaction.commit().await?;
    audit(
        &app.db,
        "pipeline_version_created",
        pipeline.id.0,
        json!({"version":version,"schema_hash":pipeline.schema_hash()}),
    )
    .await?;
    Ok(
        json!({"id":pipeline.id,"version":version,"schema_hash":pipeline.schema_hash(),"configuration":pipeline}),
    )
}

pub(crate) async fn template(app: State, id: Uuid) -> Result<Value> {
    let pipeline = repositories::pipelines::load(&app.db, id).await?;
    let mut event = RequestEvent {
        timestamp_ms: 1000,
        endpoint_id: pipeline.endpoint_id,
        method: pipeline.method.clone(),
        path: pipeline.path_template.clone(),
        headers: BTreeMap::new(),
        query: BTreeMap::new(),
        client_ip: "192.0.2.1".into(),
        body: Some(json!({})),
        body_size: 2,
        multipart: None,
    };
    let mut notes = vec![];
    for extractor in &pipeline.extractors {
        match &extractor.source {
            Source::Header { name } => {
                event.headers.insert(name.clone(), "example".into());
            }
            Source::QueryParameter { name } => {
                event.query.insert(name.clone(), "example".into());
            }
            Source::PathParameter { name } => {
                event.path = event.path.replace(&format!("{{{name}}}"), "example");
            }
            Source::JsonPointer { pointer } => notes.push(format!(
                "Supply a scalar at JSON Pointer {pointer}; numeric features require numbers."
            )),
            Source::ClientIp => (),
        }
    }
    let columns = pipeline
        .features
        .iter()
        .map(|feature| feature.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let example_row = vec!["1"; pipeline.features.len()].join(",");
    let events: Vec<_> = pipeline
        .routes()
        .enumerate()
        .map(|(i, route)| {
            let mut example = event.clone();
            example.timestamp_ms += i as u64;
            example.endpoint_id = route.endpoint_id;
            example.method = route.method.to_owned();
            example.path = route
                .path_template
                .split('/')
                .map(|segment| {
                    if segment.starts_with('{') {
                        "example"
                    } else {
                        segment
                    }
                })
                .collect::<Vec<_>>()
                .join("/");
            example
        })
        .collect();
    Ok(json!({
        "manifest": {"pipeline_version_id": pipeline.id, "schema_hash": pipeline.schema_hash()},
        "csv": format!("{columns}\n{example_row}\n"),
        "request_event": events[0],
        "request_events": events,
        "notes": notes,
        "explanation": "Request events from the selected endpoints are replayed chronologically. Include representative traffic from every endpoint in the group. History follows the pipeline's shared or per-endpoint setting. Feature vectors must match every column, in order. Provide at least 20 complete vectors; warm-up events are excluded."
    }))
}

pub(crate) async fn replay(app: State, id: Uuid, mut input: Replay) -> Result<Value> {
    let pipeline = repositories::pipelines::load(&app.db, id).await?;
    if input.events.len() > 10000 {
        return Err(bad("at most 10000 replay events"));
    }
    let model = if let Some(id) = input.model_version_id {
        let model = load_model(&app, id).await?;
        model.validate(&pipeline)?;
        Some(model)
    } else {
        None
    };
    if let Some(thresholds) = input.thresholds {
        thresholds.validate()?;
    }
    if model.is_some() && input.thresholds.is_none() {
        return Err(bad("thresholds required when testing a model"));
    }
    input.events.sort_by_key(|event| event.timestamp_ms);
    let mut store = pragma_engine::MemoryStore::default();
    let mut results = vec![];
    for event in &input.events {
        match pragma_engine::features(&pipeline, event, &app.secret, &mut store) {
            Ok(features) => {
                let decision = model
                    .as_ref()
                    .map(|model| pragma_engine::decide(model, input.thresholds.unwrap(), &features))
                    .transpose()
                    .map_err(|error| bad(error.to_string()))?;
                results.push(json!({"timestamp_ms": event.timestamp_ms, "features": features, "decision": decision}));
            }
            Err(error) => results.push(json!({
                "timestamp_ms": event.timestamp_ms,
                "decision": Decision::check(pipeline.id, error.to_string())
            })),
        }
    }
    Ok(json!({"results":results}))
}

use crate::{
    error::{Result, bad},
    repositories::{self, reporting::audit},
    state::State,
};
use pragma_domain::{Feature, Limits, Operation, Pipeline, PipelineId};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct NewApi {
    name: String,
    host: String,
}

#[derive(Deserialize)]
pub(crate) struct NewEndpoint {
    api_id: Uuid,
    name: String,
    method: String,
    path_template: String,
}

pub(crate) async fn create_api(app: State, input: NewApi) -> Result<Value> {
    let host = input.host.to_ascii_lowercase();
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(bad("name and DNS host required (no scheme or port)"));
    }
    let id = Uuid::new_v4();
    repositories::configuration::insert_api(&app.db, id, &input.name, &host).await?;
    audit(&app.db, "api_created", id, json!({})).await?;
    Ok(json!({"id":id}))
}

pub(crate) async fn create_endpoint(app: State, input: NewEndpoint) -> Result<Value> {
    if input.name.trim().is_empty() || input.name.len() > 128 {
        return Err(bad("endpoint name required"));
    }
    let mut transaction = app.db.begin().await?;
    let host = repositories::configuration::lock_api(&mut transaction, input.api_id).await?;
    let id = Uuid::new_v4();
    let pipeline = Pipeline {
        id: PipelineId(Uuid::new_v4()),
        endpoint_id: id,
        api_host: host,
        method: input.method.clone(),
        path_template: input.path_template.clone(),
        extractors: vec![],
        partitions: vec![],
        windows: vec![],
        features: vec![Feature {
            name: "body_size".into(),
            operation: Operation::BodySize,
            source: None,
            partition: None,
            window: None,
        }],
        limits: Limits::default(),
        name: String::new(),
        additional_endpoints: vec![],
        history_scope: Default::default(),
    };
    pipeline.validate()?;
    for route in
        repositories::configuration::endpoint_routes(&mut transaction, input.api_id).await?
    {
        let mut other = pipeline.clone();
        other.method = route.method;
        other.path_template = route.path_template;
        if pipeline.overlaps(&other) {
            return Err(bad("route overlaps an existing endpoint"));
        }
    }
    repositories::configuration::insert_endpoint(
        &mut transaction,
        id,
        input.api_id,
        &input.name,
        &input.method,
        &input.path_template,
    )
    .await?;
    transaction.commit().await?;
    audit(&app.db, "endpoint_created", id, json!({})).await?;
    Ok(json!({"id":id}))
}

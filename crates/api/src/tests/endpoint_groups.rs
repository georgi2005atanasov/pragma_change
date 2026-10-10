use super::support::TestContext;
use crate::{
    artifacts, data, repositories,
    services::{configuration, datasets, pipelines, releases},
};
use pragma_domain::{Endpoint, Forest, ModelId, Pipeline, PipelineId, Release, TreeNode};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn new_groups_are_rejected_but_saved_group_artifacts_remain_usable() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let api = configuration::create_api(
        context.app.clone(),
        serde_json::from_value(json!({"name":"Group", "host":"group.test"})).unwrap(),
    )
    .await
    .unwrap();
    let mut ids = vec![];
    for (method, path) in [("POST", "/orders"), ("PUT", "/payments/{id}")] {
        let endpoint = configuration::create_endpoint(
            context.app.clone(),
            serde_json::from_value(
                json!({"api_id":api["id"], "name":path, "method":method, "path_template":path}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        ids.push(serde_json::from_value::<Uuid>(endpoint["id"].clone()).unwrap());
    }
    let mut p: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    p.endpoint_id = ids[0];
    p.name = "Account activity".into();
    p.additional_endpoints.push(Endpoint {
        endpoint_id: ids[1],
        api_host: "untrusted.invalid".into(),
        method: "DELETE".into(),
        path_template: "/wrong".into(),
    });
    let error = pipelines::create_pipeline(context.app.clone(), p.clone())
        .await
        .unwrap_err();
    assert_eq!(error.0, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error.1.contains("one endpoint"));
    let mut single = p.clone();
    single.additional_endpoints.clear();
    let saved = pipelines::create_pipeline(context.app.clone(), single)
        .await
        .unwrap();
    assert_eq!(saved["configuration"]["api_host"], "group.test");
    assert!(saved["configuration"].get("additional_endpoints").is_none());

    // Seed an immutable version from before grouping was withdrawn. Reading,
    // replaying, training, and exporting old configurations must not rewrite it.
    p.id = PipelineId(Uuid::new_v4());
    p.api_host = "group.test".into();
    p.additional_endpoints[0].api_host = "group.test".into();
    p.additional_endpoints[0].method = "PUT".into();
    p.additional_endpoints[0].path_template = "/payments/{id}".into();
    p.validate().unwrap();
    let id = p.id.0;
    let mut transaction = context.app.db.begin().await.unwrap();
    repositories::pipelines::insert(&mut transaction, &p, 2)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let loaded = repositories::pipelines::load(&context.app.db, id)
        .await
        .unwrap();
    assert_eq!(loaded.additional_endpoints[0].api_host, "group.test");
    assert_eq!(loaded.additional_endpoints[0].method, "PUT");
    assert_eq!(
        loaded.additional_endpoints[0].path_template,
        "/payments/{id}"
    );
    let template = pipelines::template(context.app.clone(), id).await.unwrap();
    assert_eq!(template["request_events"].as_array().unwrap().len(), 2);
    assert_eq!(template["request_events"][1]["endpoint_id"], json!(ids[1]));
    assert_eq!(template["request_events"][1]["path"], "/payments/example");
    let content = (0..40)
        .map(|i| {
            json!({
                "timestamp_ms":i*100, "endpoint_id":ids[i%2],
                "method":if i%2==0 {"POST"} else {"PUT"},
                "path":if i%2==0 {"/orders"} else {"/payments/123"},
                "headers":{"X-Client-ID":"account-a"},
                "body":{"amount":i, "description":"Extra fields are accepted"}, "body_size":70
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let input = data::Import {
        recording_manifest: None,
        pipeline_version_id: id,
        name: "Mixed traffic".into(),
        format: "request_events".into(),
        content,
        schema_hash: loaded.schema_hash(),
    };
    let prepared = data::prepare(&loaded, &input, &[0; 32]).unwrap();
    assert!(prepared.errors.is_empty());
    assert_eq!(prepared.warmup_rows, 10);
    assert_eq!(prepared.vectors.len(), 30);
    assert_eq!(prepared.vectors[0][0], 10.);
    let dataset = datasets::import_dataset(context.app.clone(), input)
        .await
        .unwrap();
    let dataset_id = serde_json::from_value(dataset["id"].clone()).unwrap();
    let job = Uuid::new_v4();
    repositories::training_jobs::insert(&context.app.db, job, dataset_id, json!({}))
        .await
        .unwrap();
    let model = Forest {
        format_version: 1,
        id: ModelId(job),
        pipeline_version_id: loaded.id,
        schema_hash: loaded.schema_hash(),
        feature_names: loaded.features.iter().map(|f| f.name.clone()).collect(),
        max_samples: 2,
        trees: vec![vec![TreeNode {
            left: -1,
            right: -1,
            feature: -2,
            threshold: -2.,
            samples: 2,
        }]],
    };
    let bytes = serde_json::to_vec(&model).unwrap();
    let path = artifacts::write(&context.app, "group.model.json", &bytes)
        .await
        .unwrap();
    let mut transaction = context.app.db.begin().await.unwrap();
    repositories::models::insert(
        &mut transaction,
        repositories::models::NewModel {
            id: job,
            pipeline_version_id: id,
            schema_hash: &loaded.schema_hash(),
            artifact_path: &path,
            content_hash: &crate::hash::hash(&bytes),
            evaluation: &json!({}),
        },
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let models = repositories::models::list(&context.app.db).await.unwrap();
    assert_eq!(
        models[0]["pipeline_configuration"]["additional_endpoints"][0]["endpoint_id"],
        json!(ids[1])
    );
    let release = releases::create_release(context.app.clone(), serde_json::from_value(json!({
        "name":"Group release", "models":[{"model_version_id":job,"thresholds":{"check":0.8,"block":0.9}}]
    })).unwrap()).await.unwrap();
    let archive = releases::download(
        &context.app,
        serde_json::from_value(release["id"].clone()).unwrap(),
    )
    .await
    .unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()));
    let mut found = false;
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        if entry.path().unwrap().as_ref() == std::path::Path::new("release.json") {
            let release: Release = serde_json::from_reader(entry).unwrap();
            release.validate().unwrap();
            assert_eq!(release.deployments.len(), 1);
            assert_eq!(release.deployments[0].pipeline.routes().count(), 2);
            assert_eq!(release.deployments[0].model.id, model.id);
            found = true;
        }
    }
    assert!(found);
    assert_eq!(
        repositories::pipelines::load(&context.app.db, id)
            .await
            .unwrap()
            .schema_hash(),
        p.schema_hash()
    );
    context.cleanup().await;
}

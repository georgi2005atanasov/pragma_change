use super::support::TestContext;
use crate::{
    repositories::training_jobs,
    services::{configuration, datasets, pipelines, training},
};
use axum::{Json, Router, routing::post};
use pragma_domain::{Forest, ModelId, Pipeline, TreeNode};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn stale_training_attempt_cannot_publish_or_overwrite_a_reclaimed_job() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let api = configuration::create_api(
        context.app.clone(),
        serde_json::from_value(json!({
            "name": "Training fencing", "host": "fencing.test"
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let endpoint = configuration::create_endpoint(
        context.app.clone(),
        serde_json::from_value(json!({
            "api_id": api["id"], "name": "Orders", "method": "POST", "path_template": "/orders"
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let mut pipeline: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    pipeline.endpoint_id = serde_json::from_value(endpoint["id"].clone()).unwrap();
    let saved = pipelines::create_pipeline(context.app.clone(), pipeline)
        .await
        .unwrap();
    let pipeline: Pipeline = serde_json::from_value(saved["configuration"].clone()).unwrap();
    let content = format!(
        "request_count,amount_mean,body_bytes\n{}",
        "1,2,3\n".repeat(20)
    );
    let dataset = datasets::import_dataset(
        context.app.clone(),
        serde_json::from_value(json!({
            "name": "Fencing dataset", "pipeline_version_id": pipeline.id,
            "schema_hash": pipeline.schema_hash(), "format": "feature_vectors", "content": content
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let dataset_id: Uuid = serde_json::from_value(dataset["id"].clone()).unwrap();
    let job = training::train(
        context.app.clone(),
        serde_json::from_value(json!({
            "dataset_id": dataset_id
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let job_id: Uuid = serde_json::from_value(job["id"].clone()).unwrap();
    sqlx::query("UPDATE training_jobs SET status='running',attempts=2 WHERE id=$1")
        .bind(job_id)
        .execute(&context.app.db)
        .await
        .unwrap();

    let model = Forest {
        format_version: 1,
        id: ModelId(job_id),
        pipeline_version_id: pipeline.id,
        schema_hash: pipeline.schema_hash(),
        feature_names: pipeline
            .features
            .iter()
            .map(|feature| feature.name.clone())
            .collect(),
        max_samples: 2,
        trees: vec![vec![TreeNode {
            left: -1,
            right: -1,
            feature: -1,
            threshold: 0.,
            samples: 2,
        }]],
    };
    let vector = vec![1., 2., 3.];
    let score = pragma_engine::score(&model, &vector).unwrap();
    let response = json!({"model": model, "evaluation": {}, "parity_vectors": [vector], "parity_scores": [score]});
    let router = Router::new().route(
        "/v1/train",
        post(move || {
            let response = response.clone();
            async move { Json(response) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut app = context.app.clone();
    Arc::make_mut(&mut app).trainer_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let error = training::execute(&app, job_id, dataset_id, json!({}), 1)
        .await
        .unwrap_err();
    assert_eq!(error.1, "training lease superseded");
    training_jobs::record_failure(&app.db, job_id, 1, "stale failure", true)
        .await
        .unwrap();
    let state: (String, i32, Option<String>) =
        sqlx::query_as("SELECT status,attempts,error FROM training_jobs WHERE id=$1")
            .bind(job_id)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(state, ("running".into(), 2, None));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_versions WHERE id=$1")
        .bind(job_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    training::execute(&app, job_id, dataset_id, json!({}), 2)
        .await
        .unwrap();
    let model: Value = sqlx::query_scalar("SELECT to_jsonb(m) FROM model_versions m WHERE id=$1")
        .bind(job_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(model["training_job_id"], job_id.to_string());
    let state: String = sqlx::query_scalar("SELECT status FROM training_jobs WHERE id=$1")
        .bind(job_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='training_succeeded'",
    )
    .bind(job_id)
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(audits, 1);
    server.abort();
    context.cleanup().await;
}

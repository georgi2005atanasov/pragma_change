use super::support::TestContext;
use crate::{
    artifacts, repositories,
    services::{configuration, datasets, pipelines, training},
    worker,
};
use pragma_domain::Pipeline;
use serde_json::json;
use uuid::Uuid;

async fn dataset(context: &TestContext) -> Uuid {
    let api = configuration::create_api(
        context.app.clone(),
        serde_json::from_value(json!({"name":"Artifacts", "host":"artifacts.test"})).unwrap(),
    )
    .await
    .unwrap();
    let endpoint = configuration::create_endpoint(context.app.clone(), serde_json::from_value(json!({"api_id":api["id"], "name":"Orders", "method":"POST", "path_template":"/orders"})).unwrap()).await.unwrap();
    let mut p: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    p.endpoint_id = serde_json::from_value(endpoint["id"].clone()).unwrap();
    let saved = pipelines::create_pipeline(context.app.clone(), p)
        .await
        .unwrap();
    let imported = datasets::import_dataset(context.app.clone(), serde_json::from_value(json!({
        "name":"Artifact recovery", "pipeline_version_id":saved["id"], "schema_hash":saved["schema_hash"], "format":"feature_vectors",
        "content":format!("request_count,amount_mean,body_bytes\n{}", "1,2,3\n".repeat(20))
    })).unwrap()).await.unwrap();
    serde_json::from_value(imported["id"].clone()).unwrap()
}

#[tokio::test]
async fn missing_and_corrupt_inputs_fail_without_retrying_and_can_be_restored() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let id = dataset(&context).await;
    let path = context.app.artifacts.join(format!("{id}.vectors.json"));
    let original = tokio::fs::read(&path).await.unwrap();
    let queued = training::train(
        context.app.clone(),
        serde_json::from_value(json!({"dataset_id":id})).unwrap(),
    )
    .await
    .unwrap();
    let job: Uuid = serde_json::from_value(queued["id"].clone()).unwrap();
    tokio::fs::remove_file(&path).await.unwrap();
    let error = training::train(
        context.app.clone(),
        serde_json::from_value(json!({"dataset_id":id})).unwrap(),
    )
    .await
    .unwrap_err();
    assert!(error.1.contains("file is missing") && error.1.contains(&id.to_string()));
    assert!(worker::tick(&context.app).await.unwrap());
    let state: (String, i32) =
        sqlx::query_as("SELECT status,attempts FROM training_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&context.app.db)
            .await
            .unwrap();
    assert_eq!(state, ("failed".into(), 1));
    assert!(
        training::retry(context.app.clone(), job)
            .await
            .unwrap_err()
            .1
            .contains("missing")
    );
    tokio::fs::write(&path, b"[]").await.unwrap();
    assert!(
        training::retry(context.app.clone(), job)
            .await
            .unwrap_err()
            .1
            .contains("checksum mismatch")
    );
    tokio::fs::write(&path, &original).await.unwrap();
    training::retry(context.app.clone(), job).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM training_jobs WHERE id=$1")
        .bind(job)
        .fetch_one(&context.app.db)
        .await
        .unwrap();
    assert_eq!(status, "queued");
    // A checksum failure after queuing is also terminal, with a new audit entry.
    tokio::fs::write(&path, b"[]").await.unwrap();
    worker::tick(&context.app).await.unwrap();
    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='training_failed'",
    )
    .bind(job)
    .fetch_one(&context.app.db)
    .await
    .unwrap();
    assert_eq!(failures, 2);
    context.cleanup().await;
}

#[tokio::test]
async fn write_errors_identify_the_artifact_without_exposing_paths() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    tokio::fs::create_dir(context.app.artifacts.join("model.json"))
        .await
        .unwrap();
    let error = artifacts::write(&context.app, "model.json", b"{}")
        .await
        .unwrap_err();
    assert!(error.1.contains("write model.json"));
    assert!(!error.1.contains(context.app.artifacts.to_str().unwrap()));
    let mut dir = tokio::fs::read_dir(&context.app.artifacts).await.unwrap();
    while let Some(entry) = dir.next_entry().await.unwrap() {
        assert!(!entry.file_name().to_string_lossy().ends_with(".tmp"));
    }
    context.cleanup().await;
}

#[tokio::test]
async fn model_provenance_does_not_depend_on_recent_lists() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let dataset = dataset(&context).await;
    let job = Uuid::new_v4();
    repositories::training_jobs::insert(&context.app.db, job, dataset, json!({"n_estimators":12}))
        .await
        .unwrap();
    let pipeline = repositories::datasets::artifact(&context.app.db, dataset)
        .await
        .unwrap()
        .pipeline_version_id;
    sqlx::query("INSERT INTO model_versions(id,training_job_id,pipeline_version_id,schema_hash,artifact_path,content_hash,evaluation) VALUES($1,$1,$2,'hash','model.json','hash','{}')").bind(job).bind(pipeline).execute(&context.app.db).await.unwrap();
    sqlx::query("INSERT INTO training_jobs(id,dataset_id,parameters) SELECT gen_random_uuid(),$1,'{}' FROM generate_series(1,201)").bind(dataset).execute(&context.app.db).await.unwrap();
    sqlx::query("INSERT INTO datasets(id,pipeline_version_id,name,format,schema_hash,content_hash,artifact_path,row_count,validation_report) SELECT gen_random_uuid(),pipeline_version_id,'newer','feature_vectors',schema_hash,content_hash,artifact_path,row_count,validation_report FROM datasets CROSS JOIN generate_series(1,201) WHERE id=$1").bind(dataset).execute(&context.app.db).await.unwrap();
    let listed = repositories::datasets::list(&context.app.db).await.unwrap();
    assert!(
        !listed
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == dataset.to_string())
    );
    let detail = repositories::models::detail(&context.app.db, job)
        .await
        .unwrap();
    assert_eq!(detail["dataset"]["id"], dataset.to_string());
    assert_eq!(detail["dataset"]["name"], "Artifact recovery");
    assert_eq!(detail["pipeline"]["id"], pipeline.to_string());
    assert_eq!(detail["endpoint"]["name"], "Orders");
    assert_eq!(detail["training_job"]["parameters"]["n_estimators"], 12);
    assert!(
        repositories::models::detail(&context.app.db, Uuid::new_v4())
            .await
            .is_err()
    );
    context.cleanup().await;
}

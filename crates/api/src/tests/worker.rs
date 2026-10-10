use crate::{hash::hash, state::App, worker::tick};
use axum::http::StatusCode;
use pragma_domain::{Pipeline, PipelineId};
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

// Runs against the explicitly supplied disposable database, never the default DB.
#[tokio::test]
async fn retries_and_expired_leases() {
    let Some(context) = super::support::TestContext::new().await else {
        return;
    };
    let db = context.app.db.clone();
    let app_id = Uuid::new_v4();
    let endpoint = Uuid::new_v4();
    let mut p: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    p.id = PipelineId(Uuid::new_v4());
    p.endpoint_id = endpoint;
    sqlx::query("INSERT INTO apis(id,name,host) VALUES($1,$2,$2)")
        .bind(app_id)
        .bind(format!("recovery-{app_id}.test"))
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO endpoints(id, api_id, name, method, path_template) 
        VALUES($1, $2, 'recovery', 'POST', '/orders')"#,
    )
    .bind(endpoint)
    .bind(app_id)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO pipeline_versions(id, endpoint_id, version, schema_hash, configuration) 
        VALUES($1, $2, 1, $3, $4)"#,
    )
    .bind(p.id.0)
    .bind(endpoint)
    .bind(p.schema_hash())
    .bind(serde_json::to_value(&p).unwrap())
    .execute(&db)
    .await
    .unwrap();
    let dataset = Uuid::new_v4();
    let job = Uuid::new_v4();
    let dir = context.app.artifacts.clone();
    let bytes = serde_json::to_vec(&vec![vec![1., 2., 3.]; 20]).unwrap();
    tokio::fs::write(dir.join("vectors.json"), &bytes)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO datasets(id, pipeline_version_id, name, format, schema_hash,
        content_hash, artifact_path, row_count, validation_report) 
        VALUES($1, $2, 'recovery', 'feature_vectors', $3, $4, 'vectors.json',
        20, '{}')"#,
    )
    .bind(dataset)
    .bind(p.id.0)
    .bind(p.schema_hash())
    .bind(hash(&bytes))
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO training_jobs(id,dataset_id,parameters) VALUES($1,$2,$3)")
        .bind(job)
        .bind(dataset)
        .bind(json!({}))
        .execute(&db)
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let router = axum::Router::new().route(
        "/v1/train",
        axum::routing::post(move || {
            let c = counter.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                StatusCode::SERVICE_UNAVAILABLE
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let app = App {
        db: db.clone(),
        artifacts: dir.clone(),
        trainer_url: format!("http://{address}"),
        trainer_token: "test".into(),
        secret: [0; 32],
        http: reqwest::Client::new(),
    };
    for attempt in 1..=3 {
        // Advance the retry schedule in this isolated test schema.
        sqlx::query("UPDATE training_jobs SET next_run_at=now()-interval '1 second' WHERE id=$1")
            .bind(job)
            .execute(&db)
            .await
            .unwrap();
        assert!(tick(&app).await.unwrap());
        let row = sqlx::query("SELECT status,attempts,error FROM training_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(row.get::<i32, _>("attempts"), attempt);
        assert_eq!(
            row.get::<String, _>("status"),
            if attempt == 3 { "failed" } else { "queued" }
        );
        assert!(row.get::<String, _>("error").contains("503"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    sqlx::query("UPDATE training_jobs SET status='running',lease_until=now()-interval '1 minute' WHERE id=$1")
        .bind(job)
        .execute(&db)
        .await.unwrap();
    tick(&app).await.unwrap();
    let error: String = sqlx::query_scalar("SELECT error FROM training_jobs WHERE id=$1")
        .bind(job)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(error.contains("final attempt"));
    // Crash on an earlier attempt is reclaimed and increments the fencing token.
    sqlx::query(
        r#"UPDATE training_jobs 
        SET status='running', attempts=1, lease_until=now()-interval '1 minute' 
        WHERE id=$1"#,
    )
    .bind(job)
    .execute(&db)
    .await
    .unwrap();
    assert!(tick(&app).await.unwrap());
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM training_jobs WHERE id=$1")
        .bind(job)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(attempts, 2);
    sqlx::query("UPDATE training_jobs SET status='failed' WHERE id=$1")
        .bind(job)
        .execute(&db)
        .await
        .unwrap();
    server.abort();
    context.cleanup().await;
}

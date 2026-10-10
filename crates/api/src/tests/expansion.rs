use super::support::TestContext;
use axum::{Json, extract::State};
use serde_json::{Value, json};
use uuid::Uuid;
#[tokio::test]
async fn server_projection_never_returns_credentials_and_jobs_survive_storage() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO managed_servers(id,name,address,port,username,host_key,encrypted_key) VALUES($1,'test','localhost',22,'deploy','ssh-ed25519 AAAA','PRIVATE-CREDENTIAL')").bind(id).execute(&context.app.db).await.unwrap();
    let listed = crate::remote::list(State(context.app.clone()))
        .await
        .unwrap()
        .0;
    assert!(!listed.to_string().contains("PRIVATE-CREDENTIAL"));
    assert!(listed[0].get("encrypted_key").is_none());
    let request = serde_json::from_value(json!({"operation":"inspect"})).unwrap();
    let result = crate::remote::enqueue(
        State(context.app.clone()),
        axum::extract::Path(id),
        Json(request),
    )
    .await
    .unwrap()
    .0;
    let jobs = crate::remote::jobs(State(context.app.clone()))
        .await
        .unwrap()
        .0;
    assert_eq!(jobs[0]["id"], result["id"]);
    assert_eq!(jobs[0]["status"], "queued");
    let request: Value = sqlx::query_scalar("SELECT request FROM remote_jobs WHERE server_id=$1")
        .bind(id)
        .fetch_one(&context.app.db)
        .await
        .unwrap();
    assert_eq!(request["operation"], "inspect");
    context.cleanup().await;
}

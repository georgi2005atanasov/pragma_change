use super::super::{
    contracts::RecordingImport, privacy::hash, services::imports::import, state::App,
    worker::worker_tick,
};
use axum::http::HeaderMap;
use pragma_behavior::Event;
use serde_json::{Value, json};
use uuid::Uuid;
fn input(application: Uuid, recording: Uuid, content: &str) -> RecordingImport {
    serde_json::from_value(json!({"content":content,"manifest":{
        "format_version":1,"recording_id":recording,"config":{"format":"behavior_events","application_id":application},
        "pipeline_version_id":null,"schema_hash":null,"row_count":content.lines().count(),"byte_count":content.len(),"content_sha256":hash(content),"finalized":true
    }})).unwrap()
}
pub(super) async fn exercise(app: &App, headers: &HeaderMap, application: Uuid, events: &[Event]) {
    let content = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap() + "\n")
        .collect::<String>();
    let recording = Uuid::new_v4();
    let before: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(history ORDER BY partition_key),'[]') FROM behavior_state",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert!(
        import(
            app,
            &HeaderMap::new(),
            application,
            input(application, recording, &content)
        )
        .await
        .is_err()
    );
    let mut corrupt = input(application, recording, &content);
    corrupt.content.push(' ');
    assert!(import(app, headers, application, corrupt).await.is_err());
    let duplicate = format!("{content}{}\n", serde_json::to_string(&events[0]).unwrap());
    assert!(
        import(
            app,
            headers,
            application,
            input(application, recording, &duplicate)
        )
        .await
        .is_err()
    );
    assert!(
        import(
            app,
            headers,
            application,
            input(application, recording, "{}\n")
        )
        .await
        .is_err()
    );
    let absent: i64 = sqlx::query_scalar("SELECT count(*) FROM behavior_snapshots WHERE id=$1")
        .bind(recording)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(
        absent, 0,
        "invalid uploads must not leave partial snapshots"
    );
    // Race two retries. Exactly one snapshot and one audit record must result.
    let (first, second) = tokio::join!(
        import(
            app,
            headers,
            application,
            input(application, recording, &content)
        ),
        import(
            app,
            headers,
            application,
            input(application, recording, &content)
        )
    );
    assert_eq!(first.unwrap(), second.unwrap());
    let saved: Value = sqlx::query_scalar("SELECT events FROM behavior_snapshots WHERE id=$1")
        .bind(recording)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(saved.as_array().unwrap().len(), events.len());
    assert_ne!(saved[0]["actor"], events[0].actor);
    assert_eq!(saved[0]["attributes"], json!({}));
    assert_eq!(saved[0]["event_id"], json!(events[0].event_id));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM behavior_audit WHERE action='recording_imported'")
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let after: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(history ORDER BY partition_key),'[]') FROM behavior_state",
    )
    .fetch_one(&app.db)
    .await
    .unwrap();
    assert_eq!(before, after);
    let mut conflicting = input(application, recording, &content);
    conflicting
        .manifest
        .config
        .allowed_attributes
        .insert("password".into());
    assert!(
        import(app, headers, application, conflicting)
            .await
            .is_err()
    );
    let mut truncated = input(application, Uuid::new_v4(), &content);
    truncated.manifest.finalized = false;
    assert!(import(app, headers, application, truncated).await.is_err());
    // Offline outcome correlation does not require a live evaluation having occurred.
    let mut observed = events[0].clone();
    observed.correlation_id = Some(Uuid::new_v4());
    let local_content = serde_json::to_string(&observed).unwrap() + "\n";
    assert!(
        import(
            app,
            headers,
            application,
            input(application, Uuid::new_v4(), &local_content)
        )
        .await
        .is_ok()
    );
    // Exercise the real HTTP route with a recording above the normal 1 MiB event limit.
    let mut large_content = String::new();
    for i in 0..3000 {
        let mut event = events[i % events.len()].clone();
        event.event_id = Uuid::new_v4();
        large_content.push_str(&serde_json::to_string(&event).unwrap());
        large_content.push('\n');
    }
    assert!(large_content.len() > 1024 * 1024);
    let http_input = input(application, Uuid::new_v4(), &large_content);
    let payload = json!({"manifest":http_input.manifest,"content":http_input.content});
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = super::super::routes::router(std::sync::Arc::new(app.clone()));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let url = format!("http://{address}/admin/apps/{application}/imports");
    let client = reqwest::Client::new();
    let denied = client.post(&url).json(&payload).send().await.unwrap();
    assert_eq!(denied.status(), 401);
    let response = client
        .post(&url)
        .bearer_auth(&app.admin)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["event_count"], 3000);
    server.abort();

    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO behavior_training_jobs(id,snapshot_id) VALUES($1,$2)")
        .bind(job)
        .bind(recording)
        .execute(&app.db)
        .await
        .unwrap();
    worker_tick(app).await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM behavior_training_jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&app.db)
            .await
            .unwrap();
    assert_eq!(
        status, "succeeded",
        "imported recordings must train through the normal worker"
    );
}

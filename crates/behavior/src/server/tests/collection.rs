use super::super::{
    contracts::{Batch, NewApp},
    handlers::*,
    state::App,
    worker::worker_tick,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use pragma_behavior::{Event, Outcome, Policy};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
#[tokio::test]
async fn collection_is_scoped_deduplicated_private_and_trainable() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL absent: integration test skipped");
        return;
    };
    let administrator = PgPool::connect(&url).await.unwrap();
    let schema = format!("behavior_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&administrator)
        .await
        .unwrap();
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!().run(&db).await.unwrap();
    let app = Arc::new(App {
        db,
        admin: "test-admin".into(),
        secret: vec![1; 32],
    });
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer test-admin".parse().unwrap());
    let created = create(
        State(app.clone()),
        headers.clone(),
        Json(NewApp {
            name: "test".into(),
            policy: Policy::default(),
        }),
    )
    .await
    .unwrap()
    .0;
    let application = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    let mut client_headers = HeaderMap::new();
    client_headers.insert(
        "authorization",
        format!("Bearer {}", created["ingestion_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    let mut events = vec![];
    for session in 0..10 {
        for step in 0..40 {
            let event:Event=serde_json::from_value(json!({"version":1,"event_id":Uuid::new_v4(),"application_id":application,"actor":"private-user@example.test","action":if step%2==0{"read"}else{"view"},"resource":"private-document","session_id":session.to_string(),"timestamp_ms":session*100000+step*100,"outcome":"succeeded","cohort":"reader","sequence_start":step==0,"sequence_number":step,"attributes":{"password":"must-not-persist"}})).unwrap();
            events.push(event);
        }
    }
    for batch in events.chunks(100) {
        let _ = record(
            State(app.clone()),
            client_headers.clone(),
            Json(Batch {
                events: batch.to_vec(),
            }),
        )
        .await
        .unwrap();
    }
    let duplicate = record(
        State(app.clone()),
        client_headers.clone(),
        Json(Batch {
            events: vec![events[0].clone()],
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(duplicate["accepted"], 0);
    let persisted: Value = sqlx::query_scalar("SELECT event FROM behavior_events LIMIT 1")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_ne!(persisted["actor"], "private-user@example.test");
    assert_eq!(persisted["attributes"], json!({}));
    assert!(
        record(
            State(app.clone()),
            headers.clone(),
            Json(Batch {
                events: vec![events[0].clone()]
            })
        )
        .await
        .is_err()
    );
    let snap = snapshot(
        State(app.clone()),
        headers.clone(),
        Path(application),
        Json(json!({})),
    )
    .await
    .unwrap()
    .0;
    let snapshot_id = Uuid::parse_str(snap["id"].as_str().unwrap()).unwrap();
    let job = train(
        State(app.clone()),
        headers.clone(),
        Path(snapshot_id),
        Json(json!({})),
    )
    .await
    .unwrap()
    .0;
    worker_tick(&app).await.unwrap();
    let model_id = Uuid::parse_str(job["id"].as_str().unwrap()).unwrap();
    let model: Value = sqlx::query_scalar("SELECT model FROM behavior_models WHERE id=$1")
        .bind(model_id)
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert!(model["test_events"].as_u64().unwrap() >= 20);
    let _ = activate(State(app.clone()), headers.clone(), Path(model_id))
        .await
        .unwrap();
    let mut attempt = events[0].clone();
    attempt.event_id = Uuid::new_v4();
    attempt.action = "export".into();
    attempt.outcome = Outcome::Attempted;
    let first = check(
        State(app.clone()),
        client_headers.clone(),
        Json(attempt.clone()),
    )
    .await
    .unwrap()
    .0;
    let duplicate = check(
        State(app.clone()),
        client_headers.clone(),
        Json(attempt.clone()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(first, duplicate);
    assert_eq!(first["action"], "allow");
    assert_eq!(first["recommended_action"], "check");
    let mut outcome = attempt.clone();
    outcome.event_id = Uuid::new_v4();
    outcome.correlation_id = Some(attempt.event_id);
    outcome.outcome = Outcome::Succeeded;
    let mut wrong = outcome.clone();
    wrong.actor = "another-user".into();
    assert!(
        record(
            State(app.clone()),
            client_headers.clone(),
            Json(Batch {
                events: vec![wrong]
            })
        )
        .await
        .is_err()
    );
    let _ = record(
        State(app.clone()),
        client_headers.clone(),
        Json(Batch {
            events: vec![outcome.clone()],
        }),
    )
    .await
    .unwrap();
    outcome.event_id = Uuid::new_v4();
    assert!(
        record(
            State(app.clone()),
            client_headers,
            Json(Batch {
                events: vec![outcome]
            })
        )
        .await
        .is_err()
    );
    let snapshot_again = snapshot(
        State(app.clone()),
        headers.clone(),
        Path(application),
        Json(json!({"idempotency_key": snapshot_id})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(snapshot_again["content_hash"], snap["content_hash"]);
    let replay_policy = Policy {
        enforce: true,
        rules: vec![pragma_behavior::Rule {
            action: events[0].action.clone(),
            requires_success: "independent_verification".into(),
        }],
        ..Policy::default()
    };
    let _ = policy(
        State(app.clone()),
        headers.clone(),
        Path(application),
        Json(replay_policy),
    )
    .await
    .unwrap();
    let before: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(history ORDER BY partition_key),'[]') FROM behavior_state WHERE application_id=$1").bind(application).fetch_one(&app.db).await.unwrap();
    let replayed = replay(
        State(app.clone()),
        headers.clone(),
        Path(model_id),
        Json(json!({"snapshot_id": snapshot_id})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(replayed["live_state_changed"], false);
    assert_eq!(
        replayed["sample"][0]["decision"]["recommended_action"],
        "block"
    );
    assert_eq!(replayed["sample"][0]["decision"]["action"], "allow");
    let after: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(history ORDER BY partition_key),'[]') FROM behavior_state WHERE application_id=$1").bind(application).fetch_one(&app.db).await.unwrap();
    assert_eq!(before, after);
    super::recordings::exercise(&app, &headers, application, &events).await;
    let _ = purge(State(app.clone()), headers, Path(application))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM behavior_events")
        .fetch_one(&app.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    app.db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&administrator)
        .await
        .unwrap();
    administrator.close().await;
}

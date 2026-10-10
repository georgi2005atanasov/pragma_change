use super::support::TestContext;
use crate::services::{configuration, pipelines};
use axum::http::StatusCode;
use pragma_domain::Pipeline;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn overlapping_endpoints_are_serialized_and_pipeline_versions_are_unique() {
    let Some(context) = TestContext::new().await else {
        return;
    };
    let api = configuration::create_api(
        context.app.clone(),
        serde_json::from_value(json!({
            "name": "Concurrent routes", "host": "concurrency.test"
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let input = json!({"api_id": api["id"], "name": "Orders", "method": "POST", "path_template": "/orders"});
    let (first, second) = tokio::join!(
        configuration::create_endpoint(
            context.app.clone(),
            serde_json::from_value(input.clone()).unwrap()
        ),
        configuration::create_endpoint(context.app.clone(), serde_json::from_value(input).unwrap()),
    );
    let endpoint = match (first, second) {
        (Ok(endpoint), Err(error)) | (Err(error), Ok(endpoint)) => {
            assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(error.1, "route overlaps an existing endpoint");
            endpoint
        }
        outcomes => panic!("exactly one overlapping endpoint should succeed: {outcomes:?}"),
    };
    let endpoint_id: Uuid = serde_json::from_value(endpoint["id"].clone()).unwrap();
    let mut pipeline: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    pipeline.endpoint_id = endpoint_id;
    let (first, second) = tokio::join!(
        pipelines::create_pipeline(context.app.clone(), pipeline.clone()),
        pipelines::create_pipeline(context.app.clone(), pipeline),
    );
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_ne!(first["id"], second["id"]);
    let mut versions = [
        first["version"].as_i64().unwrap(),
        second["version"].as_i64().unwrap(),
    ];
    versions.sort();
    assert_eq!(versions, [1, 2]);
    for result in [first, second] {
        let id: Uuid = serde_json::from_value(result["id"].clone()).unwrap();
        let row =
            sqlx::query("SELECT configuration,schema_hash FROM pipeline_versions WHERE id=$1")
                .bind(id)
                .fetch_one(&context.app.db)
                .await
                .unwrap();
        assert_eq!(
            row.get::<Value, _>("configuration"),
            result["configuration"]
        );
        assert_eq!(row.get::<String, _>("schema_hash"), result["schema_hash"]);
        let counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM field_extractors WHERE pipeline_version_id=$1),
                    (SELECT count(*) FROM partition_definitions WHERE pipeline_version_id=$1),
                    (SELECT count(*) FROM window_definitions WHERE pipeline_version_id=$1),
                    (SELECT count(*) FROM feature_definitions WHERE pipeline_version_id=$1),
                    (SELECT count(*) FROM partition_components c JOIN partition_definitions p
                     ON c.partition_definition_id=p.id WHERE p.pipeline_version_id=$1)",
        )
        .bind(id)
        .fetch_one(&context.app.db)
        .await
        .unwrap();
        assert_eq!(counts, (2, 1, 1, 3, 1));
    }
    context.cleanup().await;
}

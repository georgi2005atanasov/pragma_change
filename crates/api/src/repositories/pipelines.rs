use crate::error::Result;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use pragma_domain::Pipeline;
use std::collections::BTreeMap;

pub(crate) async fn load(db: &PgPool, id: Uuid) -> Result<Pipeline> {
    let configuration: Value =
        sqlx::query_scalar("SELECT configuration FROM pipeline_versions WHERE id=$1")
            .bind(id)
            .fetch_one(db)
            .await?;
    Ok(serde_json::from_value(configuration)?)
}

pub(crate) async fn next_version(connection: &mut PgConnection, endpoint: Uuid) -> Result<i32> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce(max(version),0)+1 FROM pipeline_versions WHERE endpoint_id=$1",
    )
    .bind(endpoint)
    .fetch_one(connection)
    .await?)
}

pub(crate) async fn insert(
    connection: &mut PgConnection,
    pipeline: &Pipeline,
    version: i32,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO pipeline_versions(id, endpoint_id, version, schema_hash, configuration)
        VALUES($1, $2, $3, $4, $5)"#,
    )
    .bind(pipeline.id.0)
    .bind(pipeline.endpoint_id)
    .bind(version)
    .bind(pipeline.schema_hash())
    .bind(serde_json::to_value(pipeline)?)
    .execute(&mut *connection)
    .await?;
    let mut extractors = BTreeMap::new();
    let mut partitions = BTreeMap::new();
    let mut windows = BTreeMap::new();
    for extractor in &pipeline.extractors {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO field_extractors(id,pipeline_version_id,name,source) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(pipeline.id.0)
        .bind(&extractor.name)
        .bind(serde_json::to_value(&extractor.source)?)
        .execute(&mut *connection)
        .await?;
        extractors.insert(extractor.name.clone(), id);
    }
    for partition in &pipeline.partitions {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO partition_definitions(id,pipeline_version_id,name) VALUES($1,$2,$3)",
        )
        .bind(id)
        .bind(pipeline.id.0)
        .bind(&partition.name)
        .execute(&mut *connection)
        .await?;
        for (position, component) in partition.components.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO partition_components(partition_definition_id, field_extractor_id,
        position)
        VALUES($1, $2, $3)"#,
            )
            .bind(id)
            .bind(extractors[component])
            .bind(position as i32)
            .execute(&mut *connection)
            .await?;
        }
        partitions.insert(partition.name.clone(), id);
    }
    for window in &pipeline.windows {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO window_definitions(id,pipeline_version_id,name,duration_ms) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(pipeline.id.0)
        .bind(&window.name)
        .bind(window.duration_ms as i64)
        .execute(&mut *connection)
        .await?;
        windows.insert(window.name.clone(), id);
    }
    for (position, feature) in pipeline.features.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO feature_definitions(id, pipeline_version_id, name, position,
        operation, field_extractor_id, partition_definition_id, window_definition_id)
        VALUES($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(Uuid::new_v4())
        .bind(pipeline.id.0)
        .bind(&feature.name)
        .bind(position as i32)
        .bind(serde_json::to_value(feature.operation)?.as_str().unwrap())
        .bind(feature.source.as_ref().map(|source| extractors[source]))
        .bind(feature.partition.as_ref().map(|source| partitions[source]))
        .bind(feature.window.as_ref().map(|source| windows[source]))
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub(crate) async fn list(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM pipeline_versions
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

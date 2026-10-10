use crate::error::Result;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct DatasetArtifact {
    pub pipeline_version_id: Uuid,
    pub artifact_path: String,
    pub content_hash: String,
}

pub(crate) struct NewDataset<'a> {
    pub id: Uuid,
    pub pipeline_version_id: Uuid,
    pub name: &'a str,
    pub format: &'a str,
    pub schema_hash: &'a str,
    pub content_hash: &'a str,
    pub artifact_path: &'a str,
    pub row_count: i32,
    pub validation_report: &'a Value,
}

pub(crate) async fn artifact(db: &PgPool, id: Uuid) -> Result<DatasetArtifact> {
    Ok(sqlx::query_as(
        "SELECT pipeline_version_id,artifact_path,content_hash FROM datasets WHERE id=$1",
    )
    .bind(id)
    .fetch_one(db)
    .await?)
}

pub(crate) async fn insert(db: &PgPool, dataset: NewDataset<'_>) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO datasets(id, pipeline_version_id, name, format, schema_hash,
        content_hash, artifact_path, row_count, validation_report)
        VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
    )
    .bind(dataset.id)
    .bind(dataset.pipeline_version_id)
    .bind(dataset.name)
    .bind(dataset.format)
    .bind(dataset.schema_hash)
    .bind(dataset.content_hash)
    .bind(dataset.artifact_path)
    .bind(dataset.row_count)
    .bind(dataset.validation_report)
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) async fn list(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM datasets
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

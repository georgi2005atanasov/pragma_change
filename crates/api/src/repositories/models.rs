use crate::error::Result;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct ModelArtifact {
    pub artifact_path: String,
    pub content_hash: String,
}

pub(crate) struct NewModel<'a> {
    pub id: Uuid,
    pub pipeline_version_id: Uuid,
    pub schema_hash: &'a str,
    pub artifact_path: &'a str,
    pub content_hash: &'a str,
    pub evaluation: &'a Value,
}

pub(crate) async fn artifact(db: &PgPool, id: Uuid) -> Result<ModelArtifact> {
    Ok(
        sqlx::query_as("SELECT artifact_path,content_hash FROM model_versions WHERE id=$1")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}

pub(crate) async fn insert(connection: &mut PgConnection, model: NewModel<'_>) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO model_versions(id, training_job_id, pipeline_version_id, schema_hash,
        artifact_path, content_hash, evaluation)
        VALUES($1, $1, $2, $3, $4, $5, $6)
        ON CONFLICT DO NOTHING"#,
    )
    .bind(model.id)
    .bind(model.pipeline_version_id)
    .bind(model.schema_hash)
    .bind(model.artifact_path)
    .bind(model.content_hash)
    .bind(model.evaluation)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn list(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT m.*, p.endpoint_id, p.version AS pipeline_version,
                   p.configuration AS pipeline_configuration,
                   e.name AS endpoint_name, e.method, e.path_template, a.host AS api_host,
                   d.name AS dataset_name
               FROM model_versions m JOIN pipeline_versions p ON p.id=m.pipeline_version_id
               JOIN endpoints e ON e.id=p.endpoint_id JOIN apis a ON a.id=e.api_id
               JOIN training_jobs j ON j.id=m.training_job_id JOIN datasets d ON d.id=j.dataset_id
               ORDER BY m.created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

pub(crate) async fn detail(db: &PgPool, id: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT to_jsonb(m) || jsonb_build_object(
        'dataset', to_jsonb(d) - 'artifact_path', 'training_job', to_jsonb(j),
        'pipeline', to_jsonb(p), 'endpoint', to_jsonb(e), 'api', to_jsonb(a))
        FROM model_versions m JOIN training_jobs j ON j.id=m.training_job_id
        JOIN datasets d ON d.id=j.dataset_id JOIN pipeline_versions p ON p.id=m.pipeline_version_id
        JOIN endpoints e ON e.id=p.endpoint_id JOIN apis a ON a.id=e.api_id WHERE m.id=$1"#,
    )
    .bind(id)
    .fetch_one(db)
    .await?)
}

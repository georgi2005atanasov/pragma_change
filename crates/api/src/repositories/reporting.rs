use crate::error::Result;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) async fn audit(db: &PgPool, action: &str, id: Uuid, details: Value) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO audit_events(id, actor, action, entity_id, details)
        VALUES($1, 'administrator', $2, $3, $4)"#,
    )
    .bind(Uuid::new_v4())
    .bind(action)
    .bind(id)
    .bind(details)
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) async fn overview(db: &PgPool) -> Result<Value> {
    let value: Value = sqlx::query_scalar(
        r#"SELECT jsonb_build_object(
               'apis', (SELECT count(*) FROM apis),
               'endpoints', (SELECT count(*) FROM endpoints),
               'pipelines', (SELECT count(*) FROM pipeline_versions),
               'datasets', (SELECT count(*) FROM datasets),
               'models', (SELECT count(*) FROM model_versions),
               'releases', (SELECT count(*) FROM releases),
               'pending_jobs', (SELECT count(*) FROM training_jobs
                                WHERE status IN ('queued', 'running'))
           )"#,
    )
    .fetch_one(db)
    .await?;
    Ok(value)
}

pub(crate) async fn list_audits(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM audit_events
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

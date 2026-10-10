use crate::error::Result;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub(crate) async fn configuration(db: &PgPool, id: Uuid) -> Result<Value> {
    Ok(
        sqlx::query_scalar("SELECT configuration FROM releases WHERE id=$1")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}

pub(crate) async fn insert(
    connection: &mut PgConnection,
    id: Uuid,
    name: &str,
    configuration: Value,
    content_hash: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO releases(id,name,configuration,content_hash) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(name)
        .bind(configuration)
        .bind(content_hash)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn insert_model(
    connection: &mut PgConnection,
    release: Uuid,
    model: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO release_models(release_id,model_version_id) VALUES($1,$2)")
        .bind(release)
        .bind(model)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn list(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM releases
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

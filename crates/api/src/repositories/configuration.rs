use crate::error::Result;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct EndpointRoute {
    pub method: String,
    pub path_template: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EndpointConfiguration {
    pub host: String,
    pub method: String,
    pub path_template: String,
}

pub(crate) async fn insert_api(db: &PgPool, id: Uuid, name: &str, host: &str) -> Result<()> {
    sqlx::query("INSERT INTO apis(id,name,host) VALUES($1,$2,$3)")
        .bind(id)
        .bind(name)
        .bind(host)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn lock_api(connection: &mut PgConnection, id: Uuid) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT host FROM apis WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(connection)
            .await?,
    )
}

pub(crate) async fn endpoint_routes(
    connection: &mut PgConnection,
    api: Uuid,
) -> Result<Vec<EndpointRoute>> {
    Ok(
        sqlx::query_as("SELECT method,path_template FROM endpoints WHERE api_id=$1")
            .bind(api)
            .fetch_all(connection)
            .await?,
    )
}

pub(crate) async fn insert_endpoint(
    connection: &mut PgConnection,
    id: Uuid,
    api: Uuid,
    name: &str,
    method: &str,
    path: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO endpoints(id,api_id,name,method,path_template) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(api)
    .bind(name)
    .bind(method)
    .bind(path)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn lock_endpoint(
    connection: &mut PgConnection,
    id: Uuid,
) -> Result<EndpointConfiguration> {
    Ok(sqlx::query_as(
        r#"SELECT e.method, e.path_template, a.host
        FROM endpoints e
        JOIN apis a ON a.id=e.api_id
        WHERE e.id=$1
        FOR UPDATE OF e"#,
    )
    .bind(id)
    .fetch_one(connection)
    .await?)
}

pub(crate) async fn list_apis(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM apis
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

pub(crate) async fn list_endpoints(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM endpoints
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

use crate::error::Result;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct ClaimedJob {
    pub id: Uuid,
    pub dataset_id: Uuid,
    pub parameters: Value,
    pub attempts: i32,
}

pub(crate) async fn insert(db: &PgPool, id: Uuid, dataset: Uuid, parameters: Value) -> Result<()> {
    sqlx::query("INSERT INTO training_jobs(id,dataset_id,parameters) VALUES($1,$2,$3)")
        .bind(id)
        .bind(dataset)
        .bind(parameters)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn retry(db: &PgPool, id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        r#"UPDATE training_jobs
        SET status='queued', attempts=0, error=NULL, next_run_at=now(),
        updated_at=now()
        WHERE id=$1 AND status='failed'"#,
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() != 0)
}

pub(crate) async fn claim(db: &PgPool) -> Result<Option<ClaimedJob>> {
    Ok(sqlx::query_as(
        r#"WITH candidate AS (
               SELECT id FROM training_jobs
               WHERE ((status='queued' AND next_run_at<=now())
                   OR (status='running' AND lease_until<now()))
                 AND attempts<3
               ORDER BY created_at
               FOR UPDATE SKIP LOCKED
               LIMIT 1
           )
           UPDATE training_jobs j
           SET status='running', attempts=attempts+1,
               lease_until=now()+interval '6 minutes', updated_at=now()
           FROM candidate c
           WHERE j.id=c.id
           RETURNING j.id, j.dataset_id, j.parameters, j.attempts"#,
    )
    .fetch_optional(db)
    .await?)
}

pub(crate) async fn expire_final_attempts(db: &PgPool) -> Result<()> {
    sqlx::query(
        r#"WITH failed AS (
        UPDATE training_jobs
        SET status='failed', error='worker lease expired after final attempt',
        updated_at=now(), lease_until=NULL
        WHERE status='running' AND lease_until<now() AND attempts>=3 RETURNING id,error,attempts)
        INSERT INTO audit_events(id,actor,action,entity_id,details)
        SELECT gen_random_uuid(),'system','training_failed',id,jsonb_build_object('error',error,'attempts',attempts) FROM failed"#,
    )
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) async fn record_failure(
    db: &PgPool,
    id: Uuid,
    attempt: i32,
    error: &str,
    permanent: bool,
) -> Result<()> {
    let mut tx = db.begin().await?;
    let status: Option<String> = sqlx::query_scalar(
        r#"UPDATE training_jobs
        SET status=CASE WHEN $4 OR attempts>=3 THEN 'failed' ELSE 'queued' END,
        error=$2, lease_until=NULL, next_run_at=now()+interval '10 seconds'*attempts,
        updated_at=now()
        WHERE id=$1 AND status='running' AND attempts=$3 RETURNING status"#,
    )
    .bind(id)
    .bind(error)
    .bind(attempt)
    .bind(permanent)
    .fetch_optional(&mut *tx)
    .await?;
    if status.as_deref() == Some("failed") {
        sqlx::query("INSERT INTO audit_events(id,actor,action,entity_id,details) VALUES($1,'system','training_failed',$2,$3)")
            .bind(Uuid::new_v4()).bind(id).bind(serde_json::json!({"error": error, "attempts": attempt}))
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn dataset(db: &PgPool, id: Uuid) -> Result<Uuid> {
    Ok(
        sqlx::query_scalar("SELECT dataset_id FROM training_jobs WHERE id=$1")
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}

pub(crate) async fn complete(
    connection: &mut PgConnection,
    id: Uuid,
    attempt: i32,
) -> Result<bool> {
    let result = sqlx::query(
        r#"UPDATE training_jobs
        SET status='succeeded', error=NULL, lease_until=NULL, updated_at=now()
        WHERE id=$1 AND status='running' AND attempts=$2"#,
    )
    .bind(id)
    .bind(attempt)
    .execute(connection)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn list(db: &PgPool) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"SELECT coalesce(jsonb_agg(to_jsonb(t)), '[]'::jsonb)
           FROM (
               SELECT * FROM training_jobs
               ORDER BY created_at DESC
               LIMIT 200
           ) t"#,
    )
    .fetch_one(db)
    .await?)
}

use crate::error::Result;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct Administrator {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub locked: bool,
}

pub(crate) async fn administrator_count(db: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM admin_users")
        .fetch_one(db)
        .await
}

pub(crate) async fn insert_administrator(
    db: &PgPool,
    id: Uuid,
    username: &str,
    password_hash: &str,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO admin_users(id,username,password_hash) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(id)
        .bind(username)
        .bind(password_hash)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn administrator(db: &PgPool) -> Result<Administrator> {
    Ok(sqlx::query_as(
        r#"SELECT id, username, password_hash, coalesce(locked_until>now(), false) AS locked
        FROM admin_users
        LIMIT 1"#,
    )
    .fetch_one(db)
    .await?)
}

pub(crate) async fn failed_login(db: &PgPool, id: Uuid) -> Result<()> {
    sqlx::query(r#"UPDATE admin_users
        SET failed_login_count=failed_login_count+1, locked_until=CASE WHEN failed_login_count>=4 THEN now()+interval '15 minutes' ELSE NULL END
        WHERE id=$1"#)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn reset_failures(db: &PgPool, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE admin_users SET failed_login_count=0,locked_until=NULL WHERE id=$1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn delete_expired_sessions(db: &PgPool) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE expires_at<now()")
        .execute(db)
        .await?;
    Ok(())
}

pub(crate) async fn insert_session(
    db: &PgPool,
    id: Uuid,
    token_hash: &str,
    csrf_hash: &str,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO sessions(token_hash, admin_user_id, csrf_hash, expires_at)
        VALUES($1, $2, $3, now()+interval '12 hours')"#,
    )
    .bind(token_hash)
    .bind(id)
    .bind(csrf_hash)
    .execute(db)
    .await?;
    Ok(())
}

pub(crate) async fn session_csrf(db: &PgPool, token_hash: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT csrf_hash FROM sessions WHERE token_hash=$1 AND expires_at>now()",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await?)
}

pub(crate) async fn delete_session(db: &PgPool, token_hash: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
        .bind(token_hash)
        .execute(db)
        .await?;
    Ok(())
}

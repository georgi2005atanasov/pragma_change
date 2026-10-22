use crate::{
    error::{Error, Result, bad},
    hash::hash,
    repositories::{auth as repository, reporting::audit},
    state::App,
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::http::StatusCode;
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use uuid::Uuid;

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub(crate) async fn bootstrap(db: &PgPool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    if repository::administrator_count(db).await? == 0 {
        let password = std::env::var("ADMIN_PASSWORD")?;
        if password.len() < 12 {
            return Err("ADMIN_PASSWORD requires at least 12 characters".into());
        }
        let salt = SaltString::generate(&mut OsRng);
        let encoded = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|error| error.to_string())?
            .to_string();
        let username = std::env::var("ADMIN_USERNAME").unwrap_or("admin".into());
        repository::insert_administrator(db, Uuid::new_v4(), &username, &encoded).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct Login {
    username: String,
    password: String,
}

pub(crate) async fn login(app: &App, input: Login) -> Result<Value> {
    if input.password.len() > 1024 || input.username.len() > 128 {
        return Err(bad("invalid credentials"));
    }
    let administrator = repository::administrator(&app.db).await?;
    let id = administrator.id;
    if administrator.locked {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "login temporarily locked; retry in 15 minutes".into(),
        ));
    }
    let verified = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&administrator.password_hash)
            .ok()
            .is_some_and(|password| {
                Argon2::default()
                    .verify_password(input.password.as_bytes(), &password)
                    .is_ok()
            })
    })
    .await
    .unwrap_or(false);
    if !verified || administrator.username != input.username {
        repository::failed_login(&app.db, id).await?;
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    }
    repository::reset_failures(&app.db, id).await?;
    let token = random_token();
    let csrf = random_token();
    repository::delete_expired_sessions(&app.db).await?;
    repository::insert_session(&app.db, id, &hash(&token), &hash(&csrf)).await?;
    audit(&app.db, "login", id, json!({})).await?;
    Ok(json!({"token":token,"csrf_token":csrf,"expires_in":43200}))
}

pub(crate) async fn authorize(app: &App, token: Option<&str>, csrf: Option<&str>) -> Result<()> {
    let token =
        token.ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let expected = repository::session_csrf(&app.db, &hash(token))
        .await?
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "session expired".into()))?;
    if let Some(csrf) = csrf
        && !bool::from(hash(csrf).as_bytes().ct_eq(expected.as_bytes()))
    {
        return Err(Error(StatusCode::FORBIDDEN, "CSRF token required".into()));
    }
    Ok(())
}

pub(crate) async fn logout(app: &App, token: Option<&str>) -> Result<Value> {
    if let Some(token) = token {
        repository::delete_session(&app.db, &hash(token)).await?;
    }
    Ok(json!({"ok":true}))
}

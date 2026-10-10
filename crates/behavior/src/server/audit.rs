use super::{
    error::{Result, db_error},
    state::App,
};
use crate::server::repositories::audit as repository;
use serde_json::Value;
use uuid::Uuid;
pub(super) async fn audit(app: &App, id: Uuid, action: &str, details: Value) -> Result<()> {
    repository::append()
        .bind(id)
        .bind(action)
        .bind(details)
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    Ok(())
}

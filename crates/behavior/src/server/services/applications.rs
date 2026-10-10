use super::super::{
    audit::audit,
    auth::admin,
    contracts::NewApp,
    error::{Result, bad, db_error},
    privacy::hash,
    state::App,
};
use crate::server::repositories::applications as repository;
use axum::http::HeaderMap;
use pragma_behavior::Policy;
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn create(app: &App, headers: &HeaderMap, input: NewApp) -> Result<Value> {
    admin(app, headers)?;
    input.policy.validate().map_err(bad)?;
    if input.name.trim().is_empty() || input.name.len() > 128 {
        return Err(bad("invalid name"));
    }
    let id = Uuid::new_v4();
    let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    repository::create()
        .bind(id)
        .bind(input.name)
        .bind(hash(&secret))
        .bind(json!(input.policy))
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    audit(app, id, "application_created", json!({})).await?;
    Ok(json!({"id":id,"ingestion_token":secret}))
}
pub(crate) async fn list(app: &App, headers: &HeaderMap) -> Result<Value> {
    admin(app, headers)?;
    let value: Value = repository::list()
        .fetch_one(&app.db)
        .await
        .map_err(db_error)?;
    Ok(value)
}
pub(crate) async fn policy(
    app: &App,
    headers: &HeaderMap,
    id: Uuid,
    input: Policy,
) -> Result<Value> {
    admin(app, headers)?;
    input.validate().map_err(bad)?;
    repository::set_policy()
        .bind(id)
        .bind(json!(input))
        .execute(&app.db)
        .await
        .map_err(db_error)?;
    audit(app, id, "policy_updated", json!(input)).await?;
    Ok(json!({"status":"saved"}))
}
pub(crate) async fn purge(app: &App, headers: &HeaderMap, id: Uuid) -> Result<Value> {
    admin(app, headers)?;
    let mut tx = app.db.begin().await.map_err(db_error)?;
    repository::lock_application()
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    repository::clear_model()
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    repository::delete_training_jobs()
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
    for table in [
        "behavior_events",
        "behavior_state",
        "behavior_decisions",
        "behavior_models",
        "behavior_snapshots",
    ] {
        repository::delete_application_rows(table)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
    }
    tx.commit().await.map_err(db_error)?;
    audit(app, id, "application_data_deleted", json!({})).await?;
    Ok(json!({"status":"deleted"}))
}
pub(crate) async fn health(app: &App, headers: &HeaderMap, id: Uuid) -> Result<Value> {
    admin(app, headers)?;
    let recent: Vec<Value> = repository::recent_decisions()
        .bind(id)
        .fetch_all(&app.db)
        .await
        .map_err(db_error)?;
    let unusual = recent
        .iter()
        .filter(|v| {
            v["reasons"]
                .as_array()
                .is_some_and(|r| r.iter().any(|x| x == "unusual_action_transition"))
        })
        .count();
    Ok(
        json!({"recent_evaluations":recent.len(),"unusual_transitions":unusual,"review_baseline":recent.len()==100 && unusual>25,"notice":"Sustained alert-rate monitor; a change is not proof of attack and never automatically retrains a baseline."}),
    )
}

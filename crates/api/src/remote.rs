//! Durable remote administration. Secret fields never appear in public projections.
use crate::{
    error::{Result, bad},
    repositories::reporting::audit,
    secrets,
    state::{App, State},
};
use axum::{
    Json,
    extract::{Path, State as AxumState},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewServer {
    name: String,
    address: String,
    port: u16,
    username: String,
    host_key: String,
    private_key: String,
}
fn validate(input: &NewServer) -> Result<()> {
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || input.port == 0
        || input.address.is_empty()
        || input.address.len() > 253
        || !input
            .address
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
        || !input.address.as_bytes()[0].is_ascii_alphanumeric()
        || input.username == "root"
        || input.username.is_empty()
        || input.username.len() > 32
        || !input
            .username
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
        || !(input.username.as_bytes()[0].is_ascii_lowercase() || input.username.starts_with('_'))
        || input.private_key.len() > 16384
        || !input.private_key.contains("PRIVATE KEY")
    {
        return Err(bad("Invalid server details or private key"));
    }
    let parts: Vec<_> = input.host_key.split(' ').collect();
    if parts.len() != 2
        || !["ssh-ed25519", "ecdsa-sha2-nistp256", "ssh-rsa"].contains(&parts[0])
        || STANDARD.decode(parts[1]).is_err()
    {
        return Err(bad(
            "Provide a verified SSH host public key: type and base64 key only",
        ));
    }
    Ok(())
}
pub(crate) async fn create(
    AxumState(app): AxumState<State>,
    Json(input): Json<NewServer>,
) -> Result<Json<Value>> {
    validate(&input)?;
    let id = Uuid::new_v4();
    let encrypted = secrets::seal(&id.to_string(), &input.private_key)?;
    sqlx::query("INSERT INTO managed_servers(id,name,address,port,username,host_key,encrypted_key) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(input.name).bind(input.address).bind(i32::from(input.port)).bind(input.username).bind(input.host_key).bind(encrypted).execute(&app.db).await?;
    audit(&app.db, "server_registered", id, json!({})).await?;
    Ok(Json(json!({"id":id})))
}
pub(crate) async fn list(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    Ok(Json(sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(s)), '[]') FROM (SELECT id,name,address,port,username,host_key,discovery,desired_release,observed_release,verified_at,created_at FROM managed_servers ORDER BY created_at DESC LIMIT 200) s").fetch_one(&app.db).await?))
}
pub(crate) async fn jobs(AxumState(app): AxumState<State>) -> Result<Json<Value>> {
    Ok(Json(sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(j)), '[]') FROM (SELECT id,server_id,operation,status,result,batch_id,ordinal,created_at,updated_at FROM remote_jobs ORDER BY created_at DESC LIMIT 200) j").fetch_one(&app.db).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobRequest {
    operation: String,
    #[serde(default)]
    parameters: Value,
}
fn checked_request(id: Uuid, job: JobRequest) -> Result<Value> {
    let permitted: &[&str] = match job.operation.as_str() {
        "inspect" => &[],
        "preview" => &["fingerprint", "locations", "module_id"],
        "module_preview" => &["fingerprint", "module_id"],
        "stage_module" => &["package"],
        "deploy" => &["fingerprint", "preview_id", "release_id", "allow_restart"],
        "rollback" => &["fingerprint", "allow_restart"],
        "reconcile" => &["target_id"],
        _ => return Err(bad("Unsupported remote operation")),
    };
    let mut values = if job.parameters.is_null() {
        serde_json::Map::new()
    } else {
        job.parameters
            .as_object()
            .ok_or_else(|| bad("Parameters must be an object"))?
            .clone()
    };
    if values.keys().any(|k| !permitted.contains(&k.as_str())) {
        return Err(bad("Unknown operation parameter"));
    }
    for name in permitted {
        if !["preview_id", "allow_restart"].contains(name) && !values.contains_key(*name) {
            return Err(bad(format!("Missing {name}")));
        }
    }
    for name in ["preview_id", "release_id", "target_id"] {
        if let Some(value) = values.get(name) {
            Uuid::parse_str(value.as_str().ok_or_else(|| bad("Expected UUID"))?)
                .map_err(|_| bad("Invalid UUID"))?;
        }
    }
    values.insert("operation".into(), json!(job.operation));
    values.insert("operation_id".into(), json!(id));
    Ok(Value::Object(values))
}
pub(crate) async fn enqueue(
    AxumState(app): AxumState<State>,
    Path(server): Path<Uuid>,
    Json(job): Json<JobRequest>,
) -> Result<Json<Value>> {
    let id = Uuid::new_v4();
    let request = checked_request(id, job)?;
    insert_job(&app, server, id, id, 0, request).await?;
    Ok(Json(json!({"id":id, "status":"queued"})))
}
async fn insert_job(
    app: &App,
    server: Uuid,
    id: Uuid,
    batch: Uuid,
    ordinal: i32,
    request: Value,
) -> Result<()> {
    if request["operation"] == "reconcile" {
        let target = Uuid::parse_str(request["target_id"].as_str().unwrap())
            .map_err(|_| bad("Invalid target"))?;
        let owner: Option<Uuid> = sqlx::query_scalar(
            "SELECT server_id FROM remote_jobs WHERE id=$1 AND status='unknown'",
        )
        .bind(target)
        .fetch_optional(&app.db)
        .await?;
        if owner != Some(server) {
            return Err(bad(
                "Reconcile an unknown operation belonging to this server",
            ));
        }
    }
    let mut tx = app.db.begin().await?;
    if request["operation"] == "deploy" {
        let release = Uuid::parse_str(request["release_id"].as_str().unwrap())
            .map_err(|_| bad("Invalid release"))?;
        sqlx::query("UPDATE managed_servers SET desired_release=$2 WHERE id=$1")
            .bind(server)
            .bind(release)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO remote_jobs(id,server_id,operation,request,batch_id,ordinal) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(server).bind(request["operation"].as_str().unwrap()).bind(&request).bind(batch).bind(ordinal).execute(&mut *tx).await?;
    tx.commit().await?;
    audit(
        &app.db,
        "remote_operation_queued",
        id,
        json!({"server_id":server,"operation":request["operation"]}),
    )
    .await?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rollout {
    targets: Vec<RolloutTarget>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RolloutTarget {
    server_id: Uuid,
    release_id: Uuid,
    fingerprint: String,
}
pub(crate) async fn rollout(
    AxumState(app): AxumState<State>,
    Json(input): Json<Rollout>,
) -> Result<Json<Value>> {
    if input.targets.is_empty() || input.targets.len() > 100 {
        return Err(bad("Select 1–100 servers"));
    }
    let batch = Uuid::new_v4();
    let mut tx = app.db.begin().await?;
    let mut seen = std::collections::HashSet::new();
    for (ordinal, target) in input.targets.iter().enumerate() {
        if !seen.insert(target.server_id) {
            return Err(bad("Duplicate rollout server"));
        }
        let id = Uuid::new_v4();
        let request = checked_request(
            id,
            JobRequest {
                operation: "deploy".into(),
                parameters: json!({"release_id":target.release_id,"fingerprint":target.fingerprint}),
            },
        )?;
        sqlx::query("UPDATE managed_servers SET desired_release=$2 WHERE id=$1")
            .bind(target.server_id)
            .bind(target.release_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO remote_jobs(id,server_id,operation,request,batch_id,ordinal) VALUES($1,$2,'deploy',$3,$4,$5)")
            .bind(id).bind(target.server_id).bind(request).bind(batch).bind(ordinal as i32).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    audit(
        &app.db,
        "rollout_queued",
        batch,
        json!({"targets":input.targets.len()}),
    )
    .await?;
    Ok(Json(json!({"batch_id":batch})))
}
async fn execute(app: &App, server: Uuid, mut request: Value) -> Result<Value> {
    let row = sqlx::query(
        "SELECT address,port,username,host_key,encrypted_key FROM managed_servers WHERE id=$1",
    )
    .bind(server)
    .fetch_one(&app.db)
    .await?;
    let private = secrets::open(&server.to_string(), &row.get::<String, _>("encrypted_key"))?;
    if request["operation"] == "deploy" {
        let release = Uuid::parse_str(request["release_id"].as_str().unwrap())
            .map_err(|_| bad("Invalid release"))?;
        request["archive"] =
            json!(STANDARD.encode(crate::services::releases::download(app, release).await?));
        request.as_object_mut().unwrap().remove("release_id");
    }
    let payload = json!({"server":{"address":row.get::<String,_>("address"),"port":row.get::<i32,_>("port"),"username":row.get::<String,_>("username"),"host_key":row.get::<String,_>("host_key")},"private_key":private,"request":request});
    let script =
        std::env::var("REMOTE_TRANSPORT").unwrap_or_else(|_| "scripts/remote/transport.py".into());
    let mut child = tokio::process::Command::new("python3")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let bytes = serde_json::to_vec(&payload)?;
    // Write concurrently with stdout consumption to avoid pipe deadlock on large responses.
    let writer = tokio::spawn(async move { stdin.write_all(&bytes).await });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(170),
        child.wait_with_output(),
    )
    .await;
    writer.abort();
    match output {
        Ok(Ok(output)) if output.status.success() => Ok(serde_json::from_slice(&output.stdout)?),
        _ => Ok(
            json!({"status":"unknown","error":"Transport interrupted; reconcile before retrying"}),
        ),
    }
}
pub(crate) async fn tick(app: &App) -> Result<bool> {
    let mut connection = app.db.acquire().await?;
    // Transaction-scoped controller lock survives cancellation safely and serializes all rollouts.
    let mut tx = sqlx::Acquire::begin(&mut *connection).await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(734812901)")
        .fetch_one(&mut *tx)
        .await?;
    if !locked {
        return Ok(false);
    }
    sqlx::query("UPDATE remote_jobs SET status='unknown', result='{\"status\":\"unknown\",\"error\":\"Worker interrupted; reconcile the remote journal\"}',updated_at=now() WHERE status='running' AND lease_until<now()") .execute(&app.db).await?;
    sqlx::query("UPDATE remote_jobs j SET status='skipped',result='{\"error\":\"Rollout stopped at a previous failure\"}' WHERE status='queued' AND EXISTS(SELECT 1 FROM remote_jobs p WHERE p.batch_id=j.batch_id AND p.ordinal<j.ordinal AND p.status IN ('failed','unknown','skipped'))").execute(&app.db).await?;
    let job = sqlx::query("UPDATE remote_jobs SET status='running',lease_until=now()+interval '4 minutes',updated_at=now() WHERE id=(SELECT j.id FROM remote_jobs j WHERE j.status='queued' AND NOT EXISTS(SELECT 1 FROM remote_jobs p WHERE p.batch_id=j.batch_id AND p.ordinal<j.ordinal AND p.status<>'succeeded') ORDER BY created_at,ordinal LIMIT 1) RETURNING id,server_id,request,operation")
        .fetch_optional(&app.db).await?;
    let Some(job) = job else {
        return Ok(false);
    };
    let id: Uuid = job.get("id");
    let server: Uuid = job.get("server_id");
    let request: Value = job.get("request");
    let mut result = match execute(app, server, request.clone()).await {
        Ok(v) => v,
        Err(_) => {
            json!({"status":"unknown","error":"Deployment execution interrupted; verify controller configuration and reconcile"})
        }
    };
    if request["operation"] == "inspect" && result["status"] == "succeeded" {
        let previous: Option<String> =
            sqlx::query_scalar("SELECT discovery->>'fingerprint' FROM managed_servers WHERE id=$1")
                .bind(server)
                .fetch_one(&app.db)
                .await?;
        result["configuration_drift"] =
            json!(previous.is_some_and(|old| result["fingerprint"].as_str() != Some(&old)));
    }
    let status = result["status"]
        .as_str()
        .filter(|s| ["succeeded", "failed", "unknown"].contains(s))
        .unwrap_or("unknown");
    sqlx::query(
        "UPDATE remote_jobs SET status=$2,result=$3,updated_at=now(),lease_until=NULL WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(&result)
    .execute(&app.db)
    .await?;
    if status == "succeeded" {
        let observed = result["observed"]["release_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok());
        if request["operation"] == "inspect" {
            sqlx::query("UPDATE managed_servers SET discovery=$2,observed_release=$3,verified_at=now() WHERE id=$1").bind(server).bind(&result).bind(observed).execute(&app.db).await?;
        } else if result.get("observed").is_some() {
            sqlx::query("UPDATE managed_servers SET observed_release=$2,desired_release=CASE WHEN $5 THEN $2 ELSE desired_release END,verified_at=now(),discovery=coalesce(discovery,'{}') || jsonb_build_object('fingerprint',$3::text,'observed',$4::jsonb,'rollback_available',true,'configuration_drift',false) WHERE id=$1")
                .bind(server).bind(observed).bind(result["fingerprint"].as_str()).bind(&result["observed"]).bind(request["operation"] == "rollback" || request["operation"] == "reconcile").execute(&app.db).await?;
        }
    }
    if request["operation"] == "reconcile" && status != "unknown" {
        let target = Uuid::parse_str(request["target_id"].as_str().unwrap())
            .map_err(|_| bad("Invalid target"))?;
        sqlx::query("UPDATE remote_jobs SET status=$2,result=$3,updated_at=now() WHERE id=$1 AND server_id=$4 AND status='unknown'")
            .bind(target).bind(status).bind(&result).bind(server).execute(&app.db).await?;
    }
    audit(
        &app.db,
        "remote_operation_finished",
        id,
        json!({"status":status,"server_id":server}),
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}
pub(crate) async fn run(app: State) {
    loop {
        if let Err(e) = tick(&app).await {
            tracing::error!(error=%e.1, "remote worker failed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigUpload {
    content: String,
}
pub(crate) async fn analyze(Json(input): Json<ConfigUpload>) -> Result<Json<Value>> {
    if input.content.len() > 262144 {
        return Err(bad("Configuration exceeds 256 KiB"));
    }
    let transport = std::path::PathBuf::from(
        std::env::var("REMOTE_TRANSPORT").unwrap_or_else(|_| "scripts/remote/transport.py".into()),
    );
    let script = transport
        .parent()
        .ok_or_else(|| bad("Invalid transport location"))?
        .join("analyze.py");
    let mut child = tokio::process::Command::new("python3")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.content.as_bytes())
        .await?;
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait_with_output())
        .await
        .map_err(|_| bad("Configuration analysis timed out"))??;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    if let Some(error) = value.get("error") {
        return Err(bad(error.as_str().unwrap_or("Invalid configuration")));
    }
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_tools_cannot_execute_arbitrary_commands() {
        assert!(
            checked_request(
                Uuid::new_v4(),
                JobRequest {
                    operation: "shell".into(),
                    parameters: json!({"command":"id"})
                }
            )
            .is_err()
        );
        assert!(
            checked_request(
                Uuid::new_v4(),
                JobRequest {
                    operation: "inspect".into(),
                    parameters: json!({"command":"id"})
                }
            )
            .is_err()
        );
        assert!(
            checked_request(
                Uuid::new_v4(),
                JobRequest {
                    operation: "deploy".into(),
                    parameters: json!({"fingerprint":"a","release_id":"../../secret"})
                }
            )
            .is_err()
        );
    }
}

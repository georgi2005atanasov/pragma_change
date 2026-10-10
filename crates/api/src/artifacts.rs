use crate::{
    error::{Error, Result, bad},
    hash::hash,
    state::App,
};
use axum::http::StatusCode;
use std::path::Path;
use uuid::Uuid;

fn io_error(error: std::io::Error, operation: &str, resource: &str, path: &Path) -> Error {
    tracing::error!(%error, %operation, %resource, path=%path.display(), "artifact operation failed");
    if operation == "read" && error.kind() == std::io::ErrorKind::NotFound {
        bad(format!(
            "{resource} file is missing. Restore its matching artifact in ARTIFACT_DIR, or reimport the original dataset and train again."
        ))
    } else {
        Error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Could not {operation} {resource}. Check artifact storage permissions and available disk space; see the API log."
            ),
        )
    }
}

pub(crate) async fn read(
    app: &App,
    name: &str,
    expected_hash: &str,
    resource: &str,
) -> Result<Vec<u8>> {
    let path = app.artifacts.join(name);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| io_error(e, "read", resource, &path))?;
    if hash(&bytes) != expected_hash {
        tracing::error!(%resource, path=%path.display(), "artifact checksum mismatch");
        return Err(bad(format!(
            "{resource} checksum mismatch. Restore the original artifact; otherwise reimport the dataset and train again."
        )));
    }
    Ok(bytes)
}

pub(crate) async fn write(app: &App, name: &str, bytes: &[u8]) -> Result<String> {
    let path = app.artifacts.join(name);
    let temporary = app.artifacts.join(format!("{}.tmp", Uuid::new_v4()));
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|e| io_error(e, "write", name, &path))?;
    if let Err(e) = tokio::fs::rename(&temporary, &path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(io_error(e, "write", name, &path));
    }
    Ok(name.into())
}

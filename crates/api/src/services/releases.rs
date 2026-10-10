use crate::{
    error::{Result, bad},
    hash::hash,
    repositories::{self, reporting::audit},
    services::models::load_model,
    state::{App, State},
};
use pragma_domain::{Deployment, Release, Thresholds};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Selection {
    model_version_id: Uuid,
    thresholds: Thresholds,
}

#[derive(Deserialize)]
pub(crate) struct NewRelease {
    name: String,
    models: Vec<Selection>,
}

pub(crate) async fn create_release(app: State, input: NewRelease) -> Result<Value> {
    if input.name.trim().is_empty() || input.name.len() > 128 || input.models.len() > 1000 {
        return Err(bad("invalid release name or size"));
    }
    let mut release = Release {
        format_version: 1,
        release_id: Uuid::new_v4(),
        deployments: vec![],
    };
    for selection in &input.models {
        let model = load_model(&app, selection.model_version_id).await?;
        let pipeline = repositories::pipelines::load(&app.db, model.pipeline_version_id.0).await?;
        release.deployments.push(Deployment {
            pipeline,
            model,
            thresholds: selection.thresholds,
        });
    }
    release.validate()?;
    let configuration = serde_json::to_value(&release)?;
    let content_hash = hash(serde_json::to_vec(&configuration)?);
    let mut transaction = app.db.begin().await?;
    repositories::releases::insert(
        &mut transaction,
        release.release_id,
        &input.name,
        configuration,
        &content_hash,
    )
    .await?;
    for selection in &input.models {
        repositories::releases::insert_model(
            &mut transaction,
            release.release_id,
            selection.model_version_id,
        )
        .await?;
    }
    transaction.commit().await?;
    audit(&app.db, "release_created", release.release_id, json!({})).await?;
    Ok(json!({"id":release.release_id}))
}

pub(crate) async fn download(app: &App, id: Uuid) -> Result<Vec<u8>> {
    let value = repositories::releases::configuration(&app.db, id).await?;
    let release: Release = serde_json::from_value(value.clone())?;
    release.validate()?;
    let bytes = serde_json::to_vec_pretty(&value)?;
    let compatibility = json!({
        "format_version": 1,
        "module_abi": 1,
        "nginx": "1.28.0",
        "target": "x86_64-unknown-linux-gnu",
        "release_id": id,
        "partition_secret": "supply locally; never included in archive"
    });
    let files = vec![
        ("release.json", bytes),
        ("compatibility.json", serde_json::to_vec_pretty(&compatibility)?),
        ("nginx.conf.example", include_bytes!("../../../../deploy/nginx.conf").to_vec()),
        ("README.txt", b"Install the reusable module built for your Nginx binary. Supply a local 64-hex partition secret. Use scripts/deploy.py install ARCHIVE --root /etc/nginx/pragmachange, then nginx -t and reload (the script runs both). Roll back with scripts/deploy.py rollback. Training data and secrets are not included.\n".to_vec()),
    ];
    let manifest = files
        .iter()
        .map(|(name, bytes)| format!("{}  {}\n", hash(bytes), name))
        .collect::<String>();
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (name, data) in files
        .into_iter()
        .chain(std::iter::once(("SHA256SUMS", manifest.into_bytes())))
    {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive.append_data(&mut header, name, data.as_slice())?;
    }
    let bytes = archive.into_inner()?.finish()?;
    Ok(bytes)
}

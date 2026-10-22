use crate::{
    error::{Result, bad},
    hash::hash,
    repositories::{self, reporting::audit},
    services::models::load_model,
    state::{App, State},
};
use pragma_domain::{Deployment, IngressPolicy, Release, Thresholds};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DownloadTarget {
    #[default]
    Nginx,
    Apache,
}

#[derive(Deserialize)]
pub(crate) struct Selection {
    model_version_id: Uuid,
    thresholds: Thresholds,
}

#[derive(Deserialize)]
pub(crate) struct NewRelease {
    name: String,
    models: Vec<Selection>,
    #[serde(default)]
    ingress_policies: Vec<IngressPolicy>,
}

pub(crate) async fn create_release(app: State, input: NewRelease) -> Result<Value> {
    if input.name.trim().is_empty() || input.name.len() > 128 || input.models.len() > 1000 {
        return Err(bad("invalid release name or size"));
    }
    let mut release = Release {
        format_version: if input.ingress_policies.is_empty() {
            1
        } else {
            2
        },
        release_id: Uuid::new_v4(),
        deployments: vec![],
        ingress_policies: input.ingress_policies,
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
    download_for(app, id, DownloadTarget::Nginx).await
}

pub(crate) async fn download_for(app: &App, id: Uuid, target: DownloadTarget) -> Result<Vec<u8>> {
    let value = repositories::releases::configuration(&app.db, id).await?;
    let release: Release = serde_json::from_value(value.clone())?;
    release.validate()?;
    if target == DownloadTarget::Apache && !release.ingress_policies.is_empty() {
        return Err(bad("Apache does not support ingress guard releases"));
    }
    let bytes = serde_json::to_vec_pretty(&value)?;
    let compatibility = if target == DownloadTarget::Nginx {
        json!({
            "format_version": release.format_version,
            "module_abi": if release.format_version == 2 { 3 } else { 1 },
            "nginx": "1.28.0",
            "target": "x86_64-unknown-linux-gnu",
            "release_id": id,
            "partition_secret": "supply locally; never included in archive"
        })
    } else {
        json!({
            "format_version": release.format_version,
            "module_abi": 1,
            "proxy": "apache",
            "apache_api": "20120211",
            "target": "x86_64-unknown-linux-gnu",
            "packages": ["ubuntu-24.04-amd64", "debian-12-amd64"],
            "release_id": id,
            "partition_secret": "supply locally; never included in archive"
        })
    };
    let (config_name, config, instructions): (&str, &[u8], &[u8]) = match target {
        DownloadTarget::Nginx => ("nginx.conf.example", include_bytes!("../../../../deploy/nginx.conf"), b"Install the reusable module built for your Nginx binary. Supply a local 64-hex partition secret. Use scripts/deploy.py install ARCHIVE --root /etc/nginx/pragmachange, then nginx -t and reload (the script runs both). Roll back with scripts/deploy.py rollback. Training data and secrets are not included.\n"),
        DownloadTarget::Apache => ("apache.conf.example", include_bytes!("../../../../deploy/apache.conf.example"), b"Install the libapache2-mod-pragmachange package for Ubuntu 24.04 or Debian 12 (amd64). Supply a local 64-hex partition secret. Adapt apache.conf.example to your existing reverse proxy. Run pragmachange-apache validate ARCHIVE, then pragmachange-apache install ARCHIVE (append --start for first start). The helper validates, reloads and checks the serving worker's release identity. Roll back with pragmachange-apache rollback. Apache remote management is not available. Model files have the same format as Nginx; module binaries are separate. Training data and secrets are not included.\n"),
    };
    let files = vec![
        ("release.json", bytes),
        (
            "compatibility.json",
            serde_json::to_vec_pretty(&compatibility)?,
        ),
        (config_name, config.to_vec()),
        ("README.txt", instructions.to_vec()),
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

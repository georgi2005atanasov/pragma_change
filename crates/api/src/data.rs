use crate::error::{Result, bad};
use pragma_domain::{Pipeline, RequestEvent};
use pragma_engine::{MemoryStore, features};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize)]
pub struct Import {
    pub pipeline_version_id: Uuid,
    pub name: String,
    pub format: String,
    pub content: String,
    pub schema_hash: String,
    #[serde(default)]
    pub recording_manifest: Option<pragma_behavior_contract::recording::RecordingManifest>,
}
#[derive(Serialize, Deserialize)]
pub struct Prepared {
    pub source_rows: usize,
    pub warmup_rows: usize,
    pub vectors: Vec<Vec<f64>>,
    pub errors: Vec<Value>,
    pub schema_hash: String,
}
pub fn prepare(p: &Pipeline, input: &Import, secret: &[u8; 32]) -> Result<Prepared> {
    if input.schema_hash != p.schema_hash() {
        return Err(bad("schema_hash does not match this pipeline version"));
    }
    if input.name.trim().is_empty() || input.name.len() > 128 {
        return Err(bad("dataset name requires 1–128 characters"));
    }
    if let Some(manifest) = &input.recording_manifest {
        manifest.validate_content(&input.content).map_err(bad)?;
        if input.format != "request_events"
            || manifest.config.format != input.format
            || manifest.pipeline_version_id != Some(input.pipeline_version_id)
            || manifest.schema_hash.as_deref() != Some(&input.schema_hash)
            || manifest.config.endpoint_id != Some(p.endpoint_id)
        {
            return Err(bad(
                "recording does not match the selected pipeline and endpoint",
            ));
        }
    }
    let mut out = Prepared {
        source_rows: 0,
        warmup_rows: 0,
        vectors: vec![],
        errors: vec![],
        schema_hash: p.schema_hash(),
    };
    match input.format.as_str() {
        "feature_vectors" => {
            let mut reader = csv::ReaderBuilder::new()
                .flexible(false)
                .from_reader(input.content.as_bytes());
            let headers = reader.headers().map_err(|e| bad(e.to_string()))?;
            if headers.iter().collect::<Vec<_>>()
                != p.features
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            {
                return Err(bad(
                    "CSV columns must exactly match feature names and order",
                ));
            }
            for (i, row) in reader.records().enumerate() {
                out.source_rows += 1;
                let parsed = row.map_err(|e| e.to_string()).and_then(|r| {
                    r.iter()
                        .map(|s| {
                            s.parse::<f64>()
                                .map_err(|_| "invalid number".into())
                                .and_then(|x| {
                                    if x.is_finite() && (x as f32).is_finite() {
                                        Ok(x)
                                    } else {
                                        Err("non-finite number".into())
                                    }
                                })
                        })
                        .collect::<std::result::Result<Vec<_>, String>>()
                });
                match parsed {
                    Ok(v) => out.vectors.push(v),
                    Err(e) => out.errors.push(json!({"row":i+2,"error":e})),
                }
                if out.errors.len() >= 100 {
                    break;
                }
            }
        }
        "request_events" => {
            let mut events = vec![];
            for (i, line) in input.content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                out.source_rows += 1;
                match serde_json::from_str::<RequestEvent>(line) {
                    Ok(e) => events.push((i + 1, e)),
                    Err(e) => out.errors.push(json!({"row":i+1,"error":e.to_string()})),
                }
                if out.errors.len() >= 100 {
                    break;
                }
            }
            events.sort_by_key(|(row, e)| (e.timestamp_ms, *row));
            let mut store = MemoryStore::default();
            for (row, event) in events {
                match features(p, &event, secret, &mut store) {
                    Ok(f) if f.ready => out.vectors.push(f.vector),
                    Ok(_) => out.warmup_rows += 1,
                    Err(e) => out.errors.push(json!({"row":row,"error":e.to_string()})),
                }
                if out.errors.len() >= 100 {
                    break;
                }
            }
        }
        _ => return Err(bad("format must be request_events or feature_vectors")),
    }
    if out.source_rows > 100000 {
        return Err(bad("dataset exceeds 100000 rows"));
    }
    Ok(out)
}
pub fn report(p: &Pipeline, prepared: &Prepared) -> Value {
    json!({"source_rows":prepared.source_rows,"warmup_rows":prepared.warmup_rows,"training_rows":prepared.vectors.len(),"errors":prepared.errors,"schema_hash":prepared.schema_hash,"feature_names":p.features.iter().map(|f|&f.name).collect::<Vec<_>>(),"preview":prepared.vectors.iter().take(10).collect::<Vec<_>>(),"valid":prepared.errors.is_empty()&&prepared.vectors.len()>=20})
}

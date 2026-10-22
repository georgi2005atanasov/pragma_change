//! Portable on-disk recordings. Configuration selects fields; it never derives features.
use crate::Event;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingConfig {
    pub format: String,
    pub application_id: Option<Uuid>,
    pub pipeline_version_id: Option<Uuid>,
    pub schema_hash: Option<String>,
    pub endpoint_id: Option<Uuid>,
    #[serde(default)]
    pub allowed_headers: BTreeSet<String>,
    #[serde(default)]
    pub allowed_query: BTreeSet<String>,
    #[serde(default)]
    pub allowed_body_fields: BTreeSet<String>,
    #[serde(default)]
    pub allowed_attributes: BTreeSet<String>,
    #[serde(default)]
    pub include_client_ip: bool,
    #[serde(default = "default_bytes")]
    pub max_bytes: usize,
    #[serde(default = "default_records")]
    pub max_records: usize,
}
fn default_bytes() -> usize {
    MAX_BYTES
}
fn default_records() -> usize {
    100_000
}
impl RecordingConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_bytes == 0
            || self.max_bytes > MAX_BYTES
            || self.max_records == 0
            || self.max_records > 100_000
        {
            return Err("recording limits exceeded".into());
        }
        match self.format.as_str() {
            "behavior_events" if self.application_id.is_some() => {},
            "request_events" if self.endpoint_id.is_some() && self.pipeline_version_id.is_some() && self.schema_hash.as_ref().is_some_and(|h| h.len()==64 && h.bytes().all(|b| b.is_ascii_hexdigit())) => {},
            _ => return Err("behavior recording needs application_id; request recording needs endpoint_id, pipeline_version_id and the saved pipeline schema_hash".into()),
        }
        for fields in [
            &self.allowed_headers,
            &self.allowed_query,
            &self.allowed_body_fields,
            &self.allowed_attributes,
        ] {
            if fields.len() > 32
                || fields
                    .iter()
                    .any(|s| s.is_empty() || s.len() > 64 || s.chars().any(char::is_control))
            {
                return Err("invalid field allowlist".into());
            }
        }
        Ok(())
    }
    pub fn sanitize(&self, input: Value) -> Result<Value, String> {
        self.validate()?;
        let object = input.as_object().ok_or("record must be an object")?;
        if self.format == "behavior_events" {
            let mut event: Event = serde_json::from_value(input).map_err(|e| e.to_string())?;
            if Some(event.application_id) != self.application_id {
                return Err("application mismatch".into());
            }
            event
                .attributes
                .retain(|k, _| self.allowed_attributes.contains(k));
            event.validate()?;
            return serde_json::to_value(event).map_err(|e| e.to_string());
        }
        let endpoint: Uuid = serde_json::from_value(
            object
                .get("endpoint_id")
                .cloned()
                .ok_or("endpoint_id required")?,
        )
        .map_err(|e| e.to_string())?;
        if Some(endpoint) != self.endpoint_id {
            return Err("endpoint mismatch".into());
        }
        let timestamp = object
            .get("timestamp_ms")
            .and_then(Value::as_u64)
            .filter(|t| *t <= i64::MAX as u64)
            .ok_or("invalid timestamp_ms")?;
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 16 && s.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or("invalid method")?;
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| {
                s.starts_with('/')
                    && s.len() <= 2048
                    && !s.contains(['?', '#'])
                    && !s.chars().any(char::is_control)
            })
            .ok_or("path must omit query and fragment")?;
        let mut headers = serde_json::Map::new();
        if let Some(values) = object.get("headers").and_then(Value::as_object) {
            for (k, v) in values {
                let name = k.to_ascii_lowercase();
                if self
                    .allowed_headers
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&name))
                {
                    bounded_string(v)?;
                    headers.insert(name, v.clone());
                }
            }
        }
        let query = select(object.get("query"), &self.allowed_query, true)?;
        let body = select(object.get("body"), &self.allowed_body_fields, false)?;
        let client_ip = if self.include_client_ip {
            object.get("client_ip").cloned().unwrap_or(json!(""))
        } else {
            json!("")
        };
        bounded_string(&client_ip)?;
        let body_size = object
            .get("body_size")
            .and_then(Value::as_u64)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or("original body_size required")?;
        Ok(
            json!({"timestamp_ms":timestamp,"endpoint_id":endpoint,"method":method,"path":path,"headers":headers,"query":query,"body":body,"client_ip":client_ip,"body_size":body_size}),
        )
    }
}
fn bounded_string(value: &Value) -> Result<(), String> {
    if value.as_str().is_none_or(|v| v.len() > 512) {
        Err("selected string exceeds 512 bytes or has wrong type".into())
    } else {
        Ok(())
    }
}
fn select(
    value: Option<&Value>,
    allowed: &BTreeSet<String>,
    strings: bool,
) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    if let Some(values) = value.and_then(Value::as_object) {
        for (k, v) in values {
            if allowed.contains(k) {
                if strings {
                    bounded_string(v)?;
                } else if !(v.is_boolean() || v.is_number() || v.is_string())
                    || v.as_str().is_some_and(|s| s.len() > 512)
                {
                    return Err("selected body fields must be bounded scalars; flatten nested data explicitly".into());
                }
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(Value::Object(out))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingManifest {
    pub format_version: u32,
    pub recording_id: Uuid,
    pub config: RecordingConfig,
    pub pipeline_version_id: Option<Uuid>,
    pub schema_hash: Option<String>,
    pub row_count: usize,
    pub byte_count: usize,
    pub content_sha256: String,
    pub finalized: bool,
}
impl RecordingManifest {
    pub fn validate_content(&self, content: &str) -> Result<(), String> {
        self.config.validate()?;
        if self.format_version != 1
            || !self.finalized
            || self.pipeline_version_id != self.config.pipeline_version_id
            || self.schema_hash != self.config.schema_hash
        {
            return Err("invalid or unfinished recording manifest".into());
        }
        if content.len() != self.byte_count
            || self.byte_count > self.config.max_bytes
            || self.row_count == 0
            || self.row_count > self.config.max_records
            || !content.ends_with('\n')
            || content.lines().count() != self.row_count
        {
            return Err("recording size or row count mismatch".into());
        }
        if hex::encode(Sha256::digest(content.as_bytes())) != self.content_sha256 {
            return Err("recording checksum mismatch".into());
        }
        Ok(())
    }
}

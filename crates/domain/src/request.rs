use crate::MultipartMetrics;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEvent {
    pub timestamp_ms: u64,
    pub endpoint_id: Uuid,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub client_ip: String,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub body_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multipart: Option<MultipartMetrics>,
}

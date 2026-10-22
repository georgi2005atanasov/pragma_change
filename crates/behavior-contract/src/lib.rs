//! Version-one application event and decision contracts, without service dependencies.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub version: u32,
    pub event_id: Uuid,
    pub application_id: Uuid,
    pub actor: String,
    pub action: String,
    pub resource: Option<String>,
    pub session_id: String,
    pub workflow_id: Option<String>,
    pub correlation_id: Option<Uuid>,
    pub timestamp_ms: u64,
    pub outcome: Outcome,
    #[serde(default)]
    pub cohort: String,
    #[serde(default)]
    pub sequence_start: bool,
    pub sequence_number: Option<u64>,
    #[serde(default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Attempted,
    Succeeded,
    Failed,
}
impl Event {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.timestamp_ms > i64::MAX as u64 {
            return Err("invalid version or timestamp".into());
        }
        for value in [&self.actor, &self.action, &self.session_id] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err("identity, action and session must be bounded nonempty strings".into());
            }
        }
        if !self
            .action
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
        {
            return Err("action must be a stable symbolic name".into());
        }
        if self.attributes.len() > 32
            || self.cohort.len() > 128
            || self.resource.as_ref().is_some_and(|s| s.len() > 256)
            || self.workflow_id.as_ref().is_some_and(|s| s.len() > 256)
        {
            return Err("event field limit".into());
        }
        if self
            .attributes
            .keys()
            .any(|key| key.is_empty() || key.len() > 64 || key.chars().any(char::is_control))
        {
            return Err("attribute names must be bounded nonempty strings".into());
        }
        if self.attributes.values().any(|v| {
            !matches!(
                v,
                serde_json::Value::String(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::Bool(_)
            ) || v.to_string().len() > 512
        }) {
            return Err("attributes must be bounded scalar values".into());
        }
        Ok(())
    }
    pub fn partition(&self) -> String {
        serde_json::to_string(&(&self.actor, &self.session_id, &self.workflow_id)).unwrap()
    }
    pub fn symbol(&self) -> String {
        format!(
            "{}:{}",
            self.action,
            if self.outcome == Outcome::Succeeded {
                "succeeded"
            } else {
                "failed"
            }
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Decision {
    pub action: String,
    pub recommended_action: String,
    pub shadow: bool,
    pub model_id: Option<Uuid>,
    pub surprise: Option<f64>,
    pub sequence_mean_surprise: Option<f64>,
    pub baseline: Option<String>,
    pub evidence_count: u64,
    pub reasons: Vec<String>,
}
pub mod recording;

use crate::error::require;
use crate::{ModelId, PipelineId, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    pub check: f64,
    pub block: f64,
}
impl Thresholds {
    pub fn validate(self) -> Result<()> {
        require(
            self.check.is_finite()
                && self.block.is_finite()
                && self.check >= 0.
                && self.check < self.block
                && self.block <= 1.,
            "thresholds require 0 <= check < block <= 1",
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Allow,
    Check,
    Block,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub action: Action,
    pub risk_score: Option<f64>,
    pub reason: String,
    pub pipeline_version_id: PipelineId,
    pub model_version_id: Option<ModelId>,
}
impl Decision {
    pub fn check(pipeline: PipelineId, reason: impl Into<String>) -> Self {
        Self {
            action: Action::Check,
            risk_score: None,
            reason: reason.into(),
            pipeline_version_id: pipeline,
            model_version_id: None,
        }
    }
}

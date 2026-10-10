use crate::error::require;
use crate::{Forest, Pipeline, Result, Thresholds};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub pipeline: Pipeline,
    pub model: Forest,
    pub thresholds: Thresholds,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub format_version: u32,
    pub release_id: Uuid,
    pub deployments: Vec<Deployment>,
}
impl Release {
    pub fn validate(&self) -> Result<()> {
        require(
            self.format_version == 1
                && !self.deployments.is_empty()
                && self.deployments.len() <= 1000,
            "invalid release",
        )?;
        for (i, d) in self.deployments.iter().enumerate() {
            d.pipeline.validate()?;
            d.model.validate(&d.pipeline)?;
            d.thresholds.validate()?;
            for prior in &self.deployments[..i] {
                require(
                    !prior.pipeline.overlaps(&d.pipeline)
                        && !prior.pipeline.shares_endpoint(&d.pipeline),
                    "ambiguous or duplicate endpoint",
                )?;
            }
        }
        Ok(())
    }
}

use crate::error::require;
use crate::{Forest, IngressPolicy, Pipeline, Result, Thresholds};
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ingress_policies: Vec<IngressPolicy>,
}
impl Release {
    pub fn validate(&self) -> Result<()> {
        require(
            (self.format_version == 1 || self.format_version == 2)
                && (!self.deployments.is_empty() || !self.ingress_policies.is_empty())
                && self.deployments.len() <= 1000,
            "invalid release",
        )?;
        require(
            self.format_version == 2 || self.ingress_policies.is_empty(),
            "ingress policies require release format 2",
        )?;
        require(
            self.ingress_policies.len() <= 64,
            "too many ingress policies",
        )?;
        let mut names = std::collections::HashSet::new();
        for policy in &self.ingress_policies {
            policy.validate()?;
            require(names.insert(&policy.name), "duplicate ingress policy name")?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_release_can_stand_alone_but_legacy_release_requires_model() {
        let policy = IngressPolicy {
            name: "upload_guard".into(),
            max_body_bytes: 16 * 1024 * 1024,
            max_multipart_parts: 16,
            max_headers_per_part: 16,
            max_header_bytes_per_part: 8192,
            max_field_nesting: 8,
            max_file_parts: 2,
        };
        let release = Release {
            format_version: 2,
            release_id: Uuid::new_v4(),
            deployments: vec![],
            ingress_policies: vec![policy],
        };
        release.validate().unwrap();
        let mut legacy = release.clone();
        legacy.format_version = 1;
        assert!(legacy.validate().is_err());
        legacy.ingress_policies.clear();
        assert!(legacy.validate().is_err());
    }
}

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub action: String,
    pub requires_success: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub allowed_attributes: BTreeSet<String>,
    pub rules: Vec<Rule>,
    pub enforce: bool,
    pub retention_days: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed_attributes: BTreeSet::new(),
            rules: vec![],
            enforce: false,
            retention_days: 30,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=365).contains(&self.retention_days)
            || self.allowed_attributes.len() > 32
            || self.rules.len() > 64
        {
            return Err("policy bounds exceeded".into());
        }
        for rule in &self.rules {
            if rule.action.is_empty()
                || rule.requires_success.is_empty()
                || rule.action.len() > 256
                || rule.requires_success.len() > 256
            {
                return Err("invalid workflow rule".into());
            }
        }
        Ok(())
    }
}

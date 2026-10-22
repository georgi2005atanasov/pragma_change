use crate::error::require;
use crate::{Pipeline, PipelineId, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeNode {
    pub left: i32,
    pub right: i32,
    pub feature: i32,
    pub threshold: f64,
    pub samples: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forest {
    pub format_version: u32,
    pub id: ModelId,
    pub pipeline_version_id: PipelineId,
    pub schema_hash: String,
    pub feature_names: Vec<String>,
    pub max_samples: u32,
    pub trees: Vec<Vec<TreeNode>>,
}
impl Forest {
    pub fn validate(&self, pipeline: &Pipeline) -> Result<()> {
        require(
            self.format_version == 1
                && self.pipeline_version_id == pipeline.id
                && self.schema_hash == pipeline.schema_hash(),
            "incompatible model schema",
        )?;
        require(
            self.feature_names
                == pipeline
                    .features
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Vec<_>>(),
            "feature order mismatch",
        )?;
        require(
            (2..=65536).contains(&self.max_samples)
                && !self.trees.is_empty()
                && self.trees.len() <= 512,
            "invalid forest size",
        )?;
        for tree in &self.trees {
            require(
                !tree.is_empty() && tree.len() <= 131071,
                "invalid tree size",
            )?;
            let mut seen = HashSet::new();
            let mut stack = vec![(0usize, 0usize)];
            while let Some((index, depth)) = stack.pop() {
                require(
                    index < tree.len() && depth <= 64 && seen.insert(index),
                    "cyclic, shared, deep, or invalid tree node",
                )?;
                let n = &tree[index];
                require(
                    n.threshold.is_finite() && n.samples > 0 && n.samples <= self.max_samples,
                    "invalid node values",
                )?;
                if n.left == -1 && n.right == -1 {
                    continue;
                }
                require(
                    n.left >= 0
                        && n.right >= 0
                        && n.feature >= 0
                        && (n.feature as usize) < self.feature_names.len(),
                    "invalid split",
                )?;
                stack.push((n.left as usize, depth + 1));
                stack.push((n.right as usize, depth + 1));
            }
            require(seen.len() == tree.len(), "unreachable tree nodes")?;
        }
        Ok(())
    }
}

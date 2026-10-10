use crate::error::require;
use crate::{Extractor, Feature, Partition, Result, Source, Window};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PipelineId(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub endpoint_id: Uuid,
    pub api_host: String,
    pub method: String,
    pub path_template: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryScope {
    #[default]
    Shared,
    PerEndpoint,
}
impl HistoryScope {
    fn is_shared(&self) -> bool {
        *self == Self::Shared
    }
}

#[derive(Clone, Copy)]
pub struct Route<'a> {
    pub endpoint_id: Uuid,
    pub api_host: &'a str,
    pub method: &'a str,
    pub path_template: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_body_bytes: usize,
    pub max_partitions: usize,
    pub max_events_per_partition: usize,
    pub max_partition_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body_bytes: 65536,
            max_partitions: 10000,
            max_events_per_partition: 4096,
            max_partition_bytes: 1048576,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    pub id: PipelineId,
    pub endpoint_id: Uuid,
    pub api_host: String,
    pub method: String,
    pub path_template: String,
    pub extractors: Vec<Extractor>,
    pub partitions: Vec<Partition>,
    pub windows: Vec<Window>,
    pub features: Vec<Feature>,
    #[serde(default)]
    pub limits: Limits,
    // Keep legacy fields and omit defaults to preserve existing model schema hashes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_endpoints: Vec<Endpoint>,
    #[serde(default, skip_serializing_if = "HistoryScope::is_shared")]
    pub history_scope: HistoryScope,
}
fn names<'a>(items: impl Iterator<Item = &'a str>) -> Result<HashSet<&'a str>> {
    let mut seen = HashSet::new();
    for name in items {
        require(
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "names must be 1–64 snake_case characters",
        )?;
        require(seen.insert(name), format!("duplicate name: {name}"))?;
    }
    Ok(seen)
}
impl<'a> Route<'a> {
    fn validate(&self) -> Result<HashSet<&'a str>> {
        require(
            !self.api_host.is_empty()
                && self.api_host.len() <= 253
                && self
                    .api_host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'),
            "api_host must be a DNS host without a port",
        )?;
        require(
            ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"].contains(&self.method),
            "unsupported HTTP method",
        )?;
        require(
            self.path_template.starts_with('/') && !self.path_template.contains(['?', '#', '%']),
            "path_template must be an absolute decoded path",
        )?;
        let mut params = HashSet::new();
        for s in self.path_template.split('/') {
            if s.contains(['{', '}']) {
                require(
                    s.starts_with('{')
                        && s.ends_with('}')
                        && s.len() > 2
                        && !s[1..s.len() - 1].contains(['{', '}']),
                    "path parameters must occupy a whole segment",
                )?;
                require(
                    params.insert(&s[1..s.len() - 1]),
                    "duplicate path parameter",
                )?;
            }
        }
        Ok(params)
    }
    pub fn path_params(&self, path: &str) -> Option<BTreeMap<String, String>> {
        let expected: Vec<_> = self.path_template.split('/').collect();
        let actual: Vec<_> = path.split('/').collect();
        if expected.len() != actual.len() {
            return None;
        }
        let mut out = BTreeMap::new();
        for (a, b) in expected.iter().zip(actual) {
            if a.starts_with('{') {
                if b.is_empty() {
                    return None;
                }
                out.insert(a[1..a.len() - 1].to_owned(), b.to_owned());
            } else if *a != b {
                return None;
            }
        }
        Some(out)
    }
    pub fn matches(&self, host: &str, method: &str, path: &str) -> bool {
        self.api_host.eq_ignore_ascii_case(host)
            && self.method == method
            && self.path_params(path).is_some()
    }
    fn overlaps(&self, other: &Self) -> bool {
        if !self.api_host.eq_ignore_ascii_case(other.api_host) || self.method != other.method {
            return false;
        }
        let a: Vec<_> = self.path_template.split('/').collect();
        let b: Vec<_> = other.path_template.split('/').collect();
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| *x == y || x.starts_with('{') || y.starts_with('{'))
    }
}

impl Pipeline {
    pub fn routes(&self) -> impl Iterator<Item = Route<'_>> {
        std::iter::once(Route {
            endpoint_id: self.endpoint_id,
            api_host: &self.api_host,
            method: &self.method,
            path_template: &self.path_template,
        })
        .chain(self.additional_endpoints.iter().map(|e| Route {
            endpoint_id: e.endpoint_id,
            api_host: &e.api_host,
            method: &e.method,
            path_template: &e.path_template,
        }))
    }
    pub fn validate(&self) -> Result<()> {
        require(self.name.len() <= 128, "pipeline name must fit 128 bytes")?;
        require(
            self.additional_endpoints.len() < 200,
            "at most 200 endpoints per pipeline",
        )?;
        let routes: Vec<_> = self.routes().collect();
        let mut route_params = Vec::new();
        for (i, route) in routes.iter().enumerate() {
            route_params.push(route.validate()?);
            for prior in &routes[..i] {
                require(
                    prior.endpoint_id != route.endpoint_id && !prior.overlaps(route),
                    "ambiguous or duplicate endpoint in pipeline",
                )?;
            }
        }
        require(
            self.extractors.len() <= 64
                && self.partitions.len() <= 16
                && self.windows.len() <= 16
                && !self.features.is_empty()
                && self.features.len() <= 128,
            "configuration exceeds limits (64 extractors, 16 partitions/windows, 1–128 features)",
        )?;
        let extractors = names(self.extractors.iter().map(|x| x.name.as_str()))?;
        let partitions = names(self.partitions.iter().map(|x| x.name.as_str()))?;
        let windows = names(self.windows.iter().map(|x| x.name.as_str()))?;
        names(self.features.iter().map(|x| x.name.as_str()))?;
        for e in &self.extractors {
            match &e.source {
                Source::Header { name } => require(
                    !name.is_empty()
                        && name.len() <= 128
                        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
                    "invalid header name",
                )?,
                Source::JsonPointer { pointer } => require(
                    pointer.starts_with('/') && pointer.len() <= 512,
                    "JSON Pointer must start with / and fit 512 bytes",
                )?,
                Source::PathParameter { name } => require(
                    route_params
                        .iter()
                        .all(|params| params.contains(name.as_str())),
                    "path parameter must exist in every selected endpoint",
                )?,
                Source::QueryParameter { name } => require(
                    !name.is_empty() && name.len() <= 128,
                    "invalid query parameter name",
                )?,
                Source::ClientIp => (),
            }
        }
        for p in &self.partitions {
            require(
                !p.components.is_empty() && p.components.len() <= 8,
                "partition needs 1–8 components",
            )?;
            let mut seen = HashSet::new();
            for c in &p.components {
                require(
                    extractors.contains(c.as_str()) && seen.insert(c),
                    "unknown or duplicate partition component",
                )?;
            }
        }
        for w in &self.windows {
            require(
                (1..=86400000).contains(&w.duration_ms),
                "window duration must be 1 ms–24 hours",
            )?;
        }
        for f in &self.features {
            require(
                f.source.is_some() == f.operation.needs_source(),
                format!("{}: source requirement mismatch", f.name),
            )?;
            if let Some(s) = &f.source {
                require(extractors.contains(s.as_str()), "unknown feature source")?;
            }
            if f.operation.is_current() {
                require(
                    f.partition.is_none() && f.window.is_none(),
                    "current features have no partition/window",
                )?;
            } else {
                require(
                    f.partition
                        .as_ref()
                        .is_some_and(|s| partitions.contains(s.as_str())),
                    "aggregate requires a known partition",
                )?;
                require(
                    f.window
                        .as_ref()
                        .is_some_and(|s| windows.contains(s.as_str())),
                    "aggregate requires a known window",
                )?;
            }
        }
        require(
            (1..=1048576).contains(&self.limits.max_body_bytes)
                && (1..=100000).contains(&self.limits.max_partitions)
                && (2..=10000).contains(&self.limits.max_events_per_partition)
                && (1024..=4194304).contains(&self.limits.max_partition_bytes),
            "invalid runtime limits",
        )
    }
    pub fn schema_hash(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("serializable configuration"),
        ))
    }
    pub fn needs_body(&self) -> bool {
        self.extractors
            .iter()
            .any(|e| matches!(e.source, Source::JsonPointer { .. }))
    }
    pub fn path_params(&self, path: &str) -> Option<BTreeMap<String, String>> {
        self.routes().next().unwrap().path_params(path)
    }
    pub fn overlaps(&self, other: &Self) -> bool {
        self.routes()
            .any(|a| other.routes().any(|b| a.overlaps(&b)))
    }
    pub fn shares_endpoint(&self, other: &Self) -> bool {
        self.routes()
            .any(|a| other.routes().any(|b| a.endpoint_id == b.endpoint_id))
    }
}

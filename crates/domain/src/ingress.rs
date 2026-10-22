use crate::Result;
use crate::error::require;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngressPolicy {
    pub name: String,
    pub max_body_bytes: usize,
    pub max_multipart_parts: usize,
    pub max_headers_per_part: usize,
    pub max_header_bytes_per_part: usize,
    pub max_field_nesting: usize,
    pub max_file_parts: usize,
}

impl IngressPolicy {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.name.is_empty()
                && self.name.len() <= 64
                && self
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "invalid ingress policy name",
        )?;
        require(
            (1..=64 * 1024 * 1024).contains(&self.max_body_bytes)
                && (1..=4096).contains(&self.max_multipart_parts)
                && (1..=128).contains(&self.max_headers_per_part)
                && (128..=65536).contains(&self.max_header_bytes_per_part)
                && (1..=64).contains(&self.max_field_nesting)
                && (1..=4096).contains(&self.max_file_parts),
            "invalid ingress policy limits",
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultipartMetrics {
    pub parts: u32,
    pub file_parts: u32,
    pub max_part_headers: u32,
    pub max_field_nesting: u32,
    #[serde(default)]
    pub total_header_bytes: u32,
    #[serde(default)]
    pub empty_file_parts: u32,
}

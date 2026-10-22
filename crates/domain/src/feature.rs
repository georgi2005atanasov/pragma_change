use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Header { name: String },
    JsonPointer { pointer: String },
    QueryParameter { name: String },
    PathParameter { name: String },
    ClientIp,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extractor {
    pub name: String,
    pub source: Source,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    pub name: String,
    pub components: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub name: String,
    pub duration_ms: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Count,
    Rate,
    Sum,
    Mean,
    Min,
    Max,
    Stddev,
    DistinctCount,
    InterarrivalMean,
    InterarrivalStddev,
    CurrentValue,
    BodySize,
    MultipartPartCount,
    MultipartFileCount,
    MultipartMaxPartHeaders,
    MultipartMaxFieldNesting,
    MultipartTotalHeaderBytes,
    MultipartEmptyFileCount,
}
impl Operation {
    pub fn needs_source(self) -> bool {
        matches!(
            self,
            Self::Sum
                | Self::Mean
                | Self::Min
                | Self::Max
                | Self::Stddev
                | Self::DistinctCount
                | Self::CurrentValue
        )
    }
    pub fn is_current(self) -> bool {
        matches!(
            self,
            Self::CurrentValue
                | Self::BodySize
                | Self::MultipartPartCount
                | Self::MultipartFileCount
                | Self::MultipartMaxPartHeaders
                | Self::MultipartMaxFieldNesting
                | Self::MultipartTotalHeaderBytes
                | Self::MultipartEmptyFileCount
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub name: String,
    pub operation: Operation,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub partition: Option<String>,
    #[serde(default)]
    pub window: Option<String>,
}

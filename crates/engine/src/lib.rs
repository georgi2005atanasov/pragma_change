//! Deterministic extraction and window transitions. No HTTP, database, or proxy dependencies.
mod error;
mod extraction;
mod features;
mod forest;
mod partition;
mod state;

pub use error::{EngineError, EngineResult};
pub use extraction::extract;
pub use features::{FeatureResult, features};
pub use forest::{average_path_length, decide, score};
pub use partition::partition_key;
pub use state::{MemoryStore, Observation, PartitionState, StateStore};

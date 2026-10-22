use crate::{EngineError, EngineResult};
use pragma_domain::Limits;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub timestamp_ms: u64,
    pub values: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionState {
    pub observed_since_ms: u64,
    pub last_seen_ms: u64,
    pub events: VecDeque<Observation>,
}
pub trait StateStore {
    fn get(&mut self, key: &str) -> EngineResult<Option<PartitionState>>;
    fn put(&mut self, key: &str, value: PartitionState, limits: &Limits) -> EngineResult<()>;
}
#[derive(Default)]
pub struct MemoryStore {
    pub partitions: BTreeMap<String, PartitionState>,
}
impl StateStore for MemoryStore {
    fn get(&mut self, key: &str) -> EngineResult<Option<PartitionState>> {
        Ok(self.partitions.get(key).cloned())
    }
    fn put(&mut self, key: &str, value: PartitionState, limits: &Limits) -> EngineResult<()> {
        if serde_json::to_vec(&value)
            .map_err(|e| EngineError::State(e.to_string()))?
            .len()
            > limits.max_partition_bytes
        {
            return Err(EngineError::Capacity);
        }
        let namespace = key.split_once(':').map(|(id, _)| id).unwrap_or(key);
        if !self.partitions.contains_key(key)
            && self
                .partitions
                .keys()
                .filter(|k| k.starts_with(namespace))
                .count()
                >= limits.max_partitions
            && let Some(oldest) = self
                .partitions
                .iter()
                .filter(|(k, _)| k.starts_with(namespace))
                .min_by_key(|(_, s)| s.last_seen_ms)
                .map(|(k, _)| k.clone())
        {
            self.partitions.remove(&oldest);
        }
        self.partitions.insert(key.to_owned(), value);
        Ok(())
    }
}

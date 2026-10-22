use crate::EngineResult;
use hmac::{Hmac, Mac};
use pragma_domain::{HistoryScope, Partition, Pipeline};
use serde_json::Value;
use sha2::Sha256;
use std::collections::BTreeMap;
pub fn partition_key(
    p: &Pipeline,
    partition: &Partition,
    values: &BTreeMap<String, Value>,
    secret: &[u8; 32],
) -> EngineResult<String> {
    partition_key_for_endpoint(p, partition, values, secret, p.endpoint_id)
}
pub(crate) fn partition_key_for_endpoint(
    p: &Pipeline,
    partition: &Partition,
    values: &BTreeMap<String, Value>,
    secret: &[u8; 32],
    endpoint: uuid::Uuid,
) -> EngineResult<String> {
    let mut h = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC supports 32 byte keys");
    h.update(p.id.0.as_bytes());
    if p.history_scope == HistoryScope::PerEndpoint {
        h.update(endpoint.as_bytes());
    }
    for bytes in std::iter::once(partition.name.as_bytes().to_vec()).chain(
        partition
            .components
            .iter()
            .map(|c| serde_json::to_vec(&values[c]).expect("scalar JSON")),
    ) {
        h.update(&(bytes.len() as u64).to_be_bytes());
        h.update(&bytes);
    }
    Ok(format!(
        "{}:{}",
        p.id.0,
        hex::encode(h.finalize().into_bytes())
    ))
}

use crate::extraction::numeric;
use crate::partition::partition_key_for_endpoint;
use crate::{EngineError, EngineResult, Observation, PartitionState, StateStore, extract};
use pragma_domain::{Operation, Pipeline, RequestEvent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureResult {
    pub partition_keys: BTreeMap<String, String>,
    pub feature_names: Vec<String>,
    pub vector: Vec<f64>,
    pub ready: bool,
    pub reason: String,
    pub window_counts: BTreeMap<String, usize>,
}
pub fn features(
    p: &Pipeline,
    event: &RequestEvent,
    secret: &[u8; 32],
    store: &mut dyn StateStore,
) -> EngineResult<FeatureResult> {
    let values = extract(p, event)?;
    let mut result = FeatureResult {
        partition_keys: BTreeMap::new(),
        feature_names: p.features.iter().map(|f| f.name.clone()).collect(),
        vector: vec![],
        ready: true,
        reason: "ready".into(),
        window_counts: BTreeMap::new(),
    };
    let mut states = BTreeMap::new();
    let mut updates = vec![];
    for partition in &p.partitions {
        let max_window = p
            .features
            .iter()
            .filter(|f| f.partition.as_ref() == Some(&partition.name))
            .filter_map(|f| {
                p.windows
                    .iter()
                    .find(|w| Some(&w.name) == f.window.as_ref())
                    .map(|w| w.duration_ms)
            })
            .max()
            .unwrap_or(0);
        if max_window == 0 {
            continue;
        }
        let key = partition_key_for_endpoint(p, partition, &values, secret, event.endpoint_id)?;
        let mut s = store.get(&key)?.unwrap_or(PartitionState {
            observed_since_ms: event.timestamp_ms,
            last_seen_ms: event.timestamp_ms,
            events: VecDeque::new(),
        });
        if event.timestamp_ms < s.last_seen_ms {
            return Err(EngineError::OutOfOrder);
        }
        if event.timestamp_ms >= max_window {
            while s
                .events
                .front()
                .is_some_and(|o| o.timestamp_ms <= event.timestamp_ms - max_window)
            {
                s.events.pop_front();
            }
        }
        if s.events.len() >= p.limits.max_events_per_partition {
            s.events.pop_front();
            // A truncated history cannot be called complete until a full window has passed.
            s.observed_since_ms = event.timestamp_ms;
            result.reason = "state_capacity_warmup".into();
        }
        let needed: BTreeSet<_> = p
            .features
            .iter()
            .filter(|f| f.partition.as_ref() == Some(&partition.name))
            .filter_map(|f| f.source.as_ref())
            .collect();
        s.events.push_back(Observation {
            timestamp_ms: event.timestamp_ms,
            values: values
                .iter()
                .filter(|(k, _)| needed.contains(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        });
        s.last_seen_ms = event.timestamp_ms;
        if event.timestamp_ms.saturating_sub(s.observed_since_ms) < max_window {
            result.ready = false;
            if result.reason == "ready" {
                result.reason = "warmup".into();
            }
        }
        states.insert(partition.name.clone(), s.clone());
        result
            .partition_keys
            .insert(partition.name.clone(), key.clone());
        updates.push((key, s));
    }
    for f in &p.features {
        let value = match f.operation {
            Operation::BodySize => event.body_size as f64,
            Operation::CurrentValue => numeric(&values[f.source.as_ref().unwrap()]).unwrap(),
            op => {
                let duration = p
                    .windows
                    .iter()
                    .find(|w| Some(&w.name) == f.window.as_ref())
                    .unwrap()
                    .duration_ms;
                let state = &states[f.partition.as_ref().unwrap()];
                let events: Vec<_> = state
                    .events
                    .iter()
                    .filter(|o| {
                        event.timestamp_ms < duration
                            || o.timestamp_ms > event.timestamp_ms - duration
                    })
                    .collect();
                result.window_counts.insert(
                    format!(
                        "{}:{}",
                        f.partition.as_ref().unwrap(),
                        f.window.as_ref().unwrap()
                    ),
                    events.len(),
                );
                let nums: Vec<f64> = if matches!(
                    op,
                    Operation::InterarrivalMean | Operation::InterarrivalStddev
                ) {
                    events
                        .windows(2)
                        .map(|pair| (pair[1].timestamp_ms - pair[0].timestamp_ms) as f64)
                        .collect()
                } else {
                    f.source
                        .as_ref()
                        .map(|source| {
                            events
                                .iter()
                                .filter_map(|o| numeric(&o.values[source]))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let mean = if nums.is_empty() {
                    0.
                } else {
                    nums.iter().sum::<f64>() / nums.len() as f64
                };
                match op {
                    Operation::Count => events.len() as f64,
                    Operation::Rate => events.len() as f64 / (duration as f64 / 1000.),
                    Operation::Sum => nums.iter().sum(),
                    Operation::Mean | Operation::InterarrivalMean => mean,
                    Operation::Min => nums.iter().copied().reduce(f64::min).unwrap_or(0.),
                    Operation::Max => nums.iter().copied().reduce(f64::max).unwrap_or(0.),
                    Operation::Stddev | Operation::InterarrivalStddev => {
                        if nums.is_empty() {
                            0.
                        } else {
                            (nums.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                                / nums.len() as f64)
                                .sqrt()
                        }
                    }
                    Operation::DistinctCount => events
                        .iter()
                        .map(|o| o.values[f.source.as_ref().unwrap()].to_string())
                        .collect::<BTreeSet<_>>()
                        .len() as f64,
                    _ => unreachable!(),
                }
            }
        };
        if !value.is_finite() || !(value as f32).is_finite() {
            return Err(EngineError::Input(format!("non-finite feature {}", f.name)));
        }
        result.vector.push(value);
    }
    for (key, state) in updates {
        store.put(&key, state, &p.limits)?;
    }
    Ok(result)
}

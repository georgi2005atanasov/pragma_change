use crate::{Event, History};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Counts {
    pub actions: BTreeMap<String, u64>,
}
impl Counts {
    fn total(&self) -> u64 {
        self.actions.values().sum()
    }
    fn probability(&self, action: &str, vocabulary: usize) -> f64 {
        (self.actions.get(action).copied().unwrap_or(0) as f64 + 1.)
            / (self.total() as f64 + vocabulary as f64 + 1.)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Model {
    pub version: u32,
    pub id: Uuid,
    pub application_id: Uuid,
    pub vocabulary: BTreeSet<String>,
    pub tables: BTreeMap<String, Counts>,
    pub timing: BTreeMap<String, Vec<u64>>,
    pub check_surprise: f64,
    pub training_events: usize,
    pub calibration_events: usize,
    pub test_events: usize,
    pub held_out_check_rate: f64,
}
pub(crate) fn key(scope: &str, context: &[String]) -> String {
    serde_json::to_string(&(scope, context)).unwrap()
}
pub(crate) fn scopes(event: &Event) -> [String; 3] {
    [
        "global".into(),
        format!("cohort:{}", event.cohort),
        format!("actor:{}", event.actor),
    ]
}
impl Model {
    fn counts(&self, scope: &str, history: &History) -> Option<&Counts> {
        for length in (0..=history.symbols.len().min(3)).rev() {
            if let Some(counts) = self.tables.get(&key(
                scope,
                &history.symbols[history.symbols.len() - length..],
            )) && (length == 0 || counts.total() >= 5)
            {
                return Some(counts);
            }
        }
        None
    }
    pub fn probability(&self, event: &Event, history: &History) -> (f64, String, u64) {
        let scopes = scopes(event);
        let global = self.counts(&scopes[0], history);
        let peers = self
            .counts(&scopes[1], history)
            .filter(|c| c.total() >= 20)
            .or(global);
        let prior = peers.map_or(1. / (self.vocabulary.len() + 1) as f64, |c| {
            c.probability(&event.action, self.vocabulary.len())
        });
        match self.counts(&scopes[2], history) {
            Some(personal) => {
                let n = personal.total();
                let weight = n as f64 / (n as f64 + 50.);
                (
                    weight * personal.probability(&event.action, self.vocabulary.len())
                        + (1. - weight) * prior,
                    "personal_with_peer_prior".into(),
                    n,
                )
            }
            None => (
                prior,
                "peer_or_application".into(),
                peers.map_or(0, |c| c.total()),
            ),
        }
    }
}

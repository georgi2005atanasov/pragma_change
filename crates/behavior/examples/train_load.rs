//! Bounded synthetic sequence training load for the disposable two-VM benchmark.
use pragma_behavior::{Event, Model, Outcome};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
fn main() {
    let events: Vec<_> = (0..5000)
        .map(|i| Event {
            version: 1,
            event_id: uuid::Uuid::new_v4(),
            application_id: uuid::Uuid::nil(),
            actor: format!("actor-{}", i / 100),
            action: ["login", "read", "verify", "export"][i % 4].into(),
            resource: None,
            session_id: format!("session-{}", i / 100),
            workflow_id: Some(format!("workflow-{}", i / 100)),
            correlation_id: None,
            timestamp_ms: i as u64 * 1000,
            outcome: Outcome::Succeeded,
            cohort: "editor".into(),
            sequence_start: i % 100 == 0,
            sequence_number: Some(i as u64 % 100),
            attributes: BTreeMap::new(),
        })
        .collect();
    let started = Instant::now();
    let mut runs = 0;
    while started.elapsed() < Duration::from_secs(30) {
        Model::train(&events).expect("valid chronological synthetic trace");
        runs += 1;
    }
    println!("Completed {runs} synthetic sequence training runs");
}

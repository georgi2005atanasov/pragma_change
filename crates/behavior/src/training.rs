use crate::{
    Event, History, Model, Outcome,
    model::{key, scopes},
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
impl Model {
    pub fn train(events: &[Event]) -> Result<Self, String> {
        let Some(first) = events.first() else {
            return Err("no events".into());
        };
        if events.len() > 100_000
            || events
                .iter()
                .any(|e| e.application_id != first.application_id)
        {
            return Err("dataset scope or size invalid".into());
        }
        let mut sequences: BTreeMap<String, Vec<&Event>> = BTreeMap::new();
        for event in events {
            event.validate()?;
            if event.outcome != Outcome::Attempted {
                sequences.entry(event.partition()).or_default().push(event);
            }
        }
        let mut sequences: Vec<_> = sequences.into_values().collect();
        for seq in &mut sequences {
            seq.sort_by_key(|e| {
                (
                    e.timestamp_ms,
                    e.sequence_number.unwrap_or(u64::MAX),
                    e.event_id,
                )
            });
        }
        sequences.sort_by_key(|seq| seq[0].timestamp_ms);
        if sequences.len() < 5 {
            return Err("at least five complete workflows are required for separate training and calibration".into());
        }
        let split = sequences.len() * 3 / 5;
        let calibration_end = sequences.len() * 4 / 5;
        let cutoff = sequences[split][0].timestamp_ms;
        // Purge workflows crossing the chronological boundary; never split a journey.
        let training: Vec<_> = sequences[..split]
            .iter()
            .filter(|seq| seq.last().unwrap().timestamp_ms < cutoff)
            .collect();
        let mut model = Self {
            version: 1,
            id: Uuid::new_v4(),
            application_id: first.application_id,
            vocabulary: BTreeSet::new(),
            tables: BTreeMap::new(),
            timing: BTreeMap::new(),
            check_surprise: 0.,
            training_events: 0,
            calibration_events: 0,
            test_events: 0,
            held_out_check_rate: 0.,
        };
        let mut contributions: BTreeMap<String, usize> = BTreeMap::new();
        for seq in training {
            let mut history = History::default();
            for event in seq {
                history.prepare(event);
                let count = contributions.entry(event.actor.clone()).or_default();
                if *count >= 1000 {
                    break;
                }
                if history.complete {
                    *count += 1;
                    model.training_events += 1;
                    model.vocabulary.insert(event.action.clone());
                    if model.vocabulary.len() > 512 {
                        return Err(
                            "action vocabulary exceeds 512; use stable operation names".into()
                        );
                    }
                    for scope in scopes(event) {
                        for length in 0..=history.symbols.len().min(3) {
                            *model
                                .tables
                                .entry(key(
                                    &scope,
                                    &history.symbols[history.symbols.len() - length..],
                                ))
                                .or_default()
                                .actions
                                .entry(event.action.clone())
                                .or_default() += 1;
                        }
                    }
                    if let (Some(previous), Some(last)) = (history.symbols.last(), history.last_ms)
                    {
                        let timings = model
                            .timing
                            .entry(key(previous, std::slice::from_ref(&event.action)))
                            .or_default();
                        if timings.len() < 10000 {
                            timings.push(event.timestamp_ms.saturating_sub(last));
                        }
                    }
                }
                history.observe(event);
            }
        }
        if model.training_events < 100 {
            return Err("at least 100 complete training transitions required after chronological purge and actor caps".into());
        }
        let mut scores = vec![];
        for seq in &sequences[split..calibration_end] {
            if seq.last().unwrap().timestamp_ms >= sequences[calibration_end][0].timestamp_ms {
                continue;
            }
            let mut history = History::default();
            for event in seq {
                history.prepare(event);
                if history.complete {
                    scores.push(-model.probability(event, &history).0.ln());
                }
                history.observe(event);
            }
        }
        if scores.len() < 20 {
            return Err("at least 20 independent calibration transitions required".into());
        }
        scores.sort_by(f64::total_cmp);
        model.calibration_events = scores.len();
        model.check_surprise = scores[((scores.len() - 1) as f64 * 0.99).ceil() as usize];
        for timings in model.timing.values_mut() {
            timings.sort_unstable();
        }
        let mut checks = 0;
        for seq in &sequences[calibration_end..] {
            let mut history = History::default();
            for event in seq {
                history.prepare(event);
                if history.complete {
                    model.test_events += 1;
                    if -model.probability(event, &history).0.ln() > model.check_surprise {
                        checks += 1;
                    }
                }
                history.observe(event);
            }
        }
        if model.test_events < 20 {
            return Err("at least 20 independent test transitions required".into());
        }
        model.held_out_check_rate = checks as f64 / model.test_events as f64;
        Ok(model)
    }
}

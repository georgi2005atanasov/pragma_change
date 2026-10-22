use crate::{Event, Outcome};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub symbols: Vec<String>,
    pub last_ms: Option<u64>,
    pub last_sequence: Option<u64>,
    pub complete: bool,
    #[serde(default)]
    pub surprises: Vec<f64>,
}
impl History {
    /// Validate the next transition before either learning or scoring it.
    pub fn prepare(&mut self, event: &Event) {
        if event.sequence_start {
            *self = Self {
                complete: true,
                ..Self::default()
            };
        }
        if self.last_ms.is_some_and(|t| {
            event.timestamp_ms < t || (event.timestamp_ms == t && event.sequence_number.is_none())
        }) || self
            .last_sequence
            .is_some_and(|previous| event.sequence_number != previous.checked_add(1))
        {
            self.complete = false;
        }
    }
    pub fn observe(&mut self, event: &Event) {
        if event.outcome == Outcome::Attempted {
            return;
        }
        let backwards =
            self.last_ms.is_some_and(|t| event.timestamp_ms < t) && !event.sequence_start;
        self.prepare(event);
        if backwards {
            return;
        }
        self.symbols.push(event.symbol());
        if self.symbols.len() > 128 {
            self.symbols.remove(0);
            self.complete = false;
        }
        self.last_ms = Some(event.timestamp_ms);
        self.last_sequence = event.sequence_number;
    }
}

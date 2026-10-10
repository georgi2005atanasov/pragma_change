//! Bounded symbolic sequence detection. Scores are surprise, not attack probability.
mod history;
mod model;
mod policy;
mod scoring;
mod training;
pub use history::History;
pub use model::{Counts, Model};
pub use policy::{Policy, Rule};
pub use pragma_behavior_contract::{Decision, Event, Outcome};
pub use scoring::evaluate;
#[cfg(test)]
mod tests;

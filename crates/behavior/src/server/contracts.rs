use pragma_behavior::{Event, Policy};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewApp {
    pub name: String,
    #[serde(default)]
    pub policy: Policy,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Batch {
    pub events: Vec<Event>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecordingImport {
    pub manifest: pragma_behavior_contract::recording::RecordingManifest,
    pub content: String,
}

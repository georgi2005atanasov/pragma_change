use super::state::App;
use hmac::{Hmac, Mac};
use pragma_behavior::{Event, Policy};
use sha2::{Digest, Sha256};
use uuid::Uuid;
pub(super) fn hash(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}
pub(super) fn pseudonym(app: &App, application: Uuid, kind: &str, value: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(&app.secret).unwrap();
    mac.update(
        serde_json::to_string(&(application, kind, value))
            .unwrap()
            .as_bytes(),
    );
    hex::encode(mac.finalize().into_bytes())
}
pub(super) fn sanitize(app: &App, policy: &Policy, mut event: Event) -> Event {
    event.actor = pseudonym(app, event.application_id, "actor", &event.actor);
    event.session_id = pseudonym(app, event.application_id, "session", &event.session_id);
    event.workflow_id = event
        .workflow_id
        .map(|v| pseudonym(app, event.application_id, "workflow", &v));
    event.resource = event
        .resource
        .map(|v| pseudonym(app, event.application_id, "resource", &v));
    event
        .attributes
        .retain(|k, _| policy.allowed_attributes.contains(k));
    event
}

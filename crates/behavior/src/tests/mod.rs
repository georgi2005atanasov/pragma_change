use super::*;
use std::collections::BTreeMap;
use uuid::Uuid;
fn event(action: &str, outcome: Outcome) -> Event {
    Event {
        version: 1,
        event_id: Uuid::new_v4(),
        application_id: Uuid::nil(),
        actor: "actor".into(),
        action: action.into(),
        resource: None,
        session_id: "session".into(),
        workflow_id: None,
        correlation_id: None,
        timestamp_ms: 1000,
        outcome,
        cohort: "reader".into(),
        sequence_start: true,
        sequence_number: None,
        attributes: BTreeMap::new(),
    }
}
#[test]
fn gaps_are_incomplete_before_scoring_or_learning() {
    let mut first = event("login", Outcome::Succeeded);
    first.sequence_number = Some(1);
    let mut history = History::default();
    history.observe(&first);
    let mut next = event("export", Outcome::Succeeded);
    next.sequence_start = false;
    next.timestamp_ms += 1;
    for sequence in [None, Some(1), Some(3)] {
        next.sequence_number = sequence;
        let mut context = history.clone();
        context.prepare(&next);
        assert!(!context.complete);
        assert!(
            evaluate(None, &Policy::default(), &next, &history)
                .reasons
                .contains(&"insufficient_or_incomplete_history".into())
        );
    }
    next.sequence_number = Some(2);
    history.prepare(&next);
    assert!(history.complete);
}

#[test]
fn failed_attempt_does_not_satisfy_success_rule() {
    let policy = Policy {
        enforce: true,
        rules: vec![Rule {
            action: "export".into(),
            requires_success: "verify".into(),
        }],
        ..Policy::default()
    };
    let mut history = History::default();
    history.observe(&event("verify", Outcome::Attempted));
    assert!(history.symbols.is_empty());
    history.observe(&event("verify", Outcome::Failed));
    let mut next = event("export", Outcome::Attempted);
    next.sequence_start = false;
    next.timestamp_ms += 1;
    assert_eq!(evaluate(None, &policy, &next, &history).action, "block");
    history.observe(&event("verify", Outcome::Succeeded));
    assert_ne!(evaluate(None, &policy, &next, &history).action, "block");
}
#[test]
fn incomplete_history_checks_and_shadow_never_blocks() {
    let mut event = event("export", Outcome::Attempted);
    event.sequence_start = false;
    let policy = Policy {
        enforce: true,
        ..Policy::default()
    };
    assert_eq!(
        evaluate(None, &policy, &event, &History::default()).action,
        "check"
    );
    assert_eq!(
        evaluate(None, &Policy::default(), &event, &History::default()).action,
        "allow"
    );
}
#[test]
fn sequences_train_with_backoff_and_new_actor() {
    let mut data = vec![];
    for session in 0..10 {
        for step in 0..40 {
            let mut e = event(
                if step % 2 == 0 { "read" } else { "view" },
                Outcome::Succeeded,
            );
            e.session_id = session.to_string();
            e.actor = format!("actor-{}", session % 3);
            e.timestamp_ms = session * 100000 + step * 100;
            e.sequence_start = step == 0;
            data.push(e);
        }
    }
    let model = Model::train(&data).unwrap();
    let mut unusual = event("export", Outcome::Attempted);
    unusual.actor = "new-user".into();
    let p = model.probability(&unusual, &History::default()).0;
    assert!(p > 0. && p < 1.);
    let decision = evaluate(
        Some(&model),
        &Policy::default(),
        &unusual,
        &History::default(),
    );
    assert_eq!(decision.recommended_action, "check");
    assert!(decision.surprise.unwrap().is_finite());
}
#[test]
fn gap_and_backwards_time_mark_history_incomplete() {
    let mut h = History::default();
    let mut e = event("read", Outcome::Succeeded);
    e.sequence_number = Some(1);
    h.observe(&e);
    e.sequence_start = false;
    e.sequence_number = Some(3);
    h.observe(&e);
    assert!(!h.complete);
}

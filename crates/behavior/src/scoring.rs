use crate::model::key;
use crate::{Decision, Event, History, Model, Policy};
pub fn evaluate(
    model: Option<&Model>,
    policy: &Policy,
    event: &Event,
    history: &History,
) -> Decision {
    let mut result = Decision {
        action: "allow".into(),
        recommended_action: "allow".into(),
        shadow: !policy.enforce,
        model_id: model.map(|m| m.id),
        surprise: None,
        sequence_mean_surprise: None,
        baseline: None,
        evidence_count: 0,
        reasons: vec![],
    };
    let mut context = history.clone();
    context.prepare(event);
    let history = &context;
    let complete = history.complete;
    if !complete || model.is_none() {
        result.recommended_action = "check".into();
        result
            .reasons
            .push("insufficient_or_incomplete_history".into());
    }
    for rule in &policy.rules {
        if event.action == rule.action
            && !history
                .symbols
                .contains(&format!("{}:succeeded", rule.requires_success))
        {
            result.recommended_action = if complete { "block" } else { "check" }.into();
            result.reasons.push(format!(
                "required_success_missing:{}",
                rule.requires_success
            ));
        }
    }
    if let Some(model) = model.filter(|_| complete) {
        let (probability, baseline, n) = model.probability(event, history);
        let surprise = -probability.ln();
        result.surprise = Some(surprise);
        result.baseline = Some(baseline);
        result.evidence_count = n;
        result.sequence_mean_surprise = Some(
            (history.surprises.iter().sum::<f64>() + surprise)
                / (history.surprises.len() + 1) as f64,
        );
        if surprise > model.check_surprise {
            result.reasons.push("unusual_action_transition".into());
            if result.recommended_action == "allow" {
                result.recommended_action = "check".into();
            }
        }
        if let (Some(previous), Some(last)) = (history.symbols.last(), history.last_ms)
            && let Some(times) = model
                .timing
                .get(&key(previous, std::slice::from_ref(&event.action)))
                .filter(|v| v.len() >= 20)
        {
            let gap = event.timestamp_ms.saturating_sub(last);
            if gap < times[times.len() / 100] || gap > times[(times.len() - 1) * 99 / 100] {
                result.reasons.push("unusual_transition_timing".into());
                if result.recommended_action == "allow" {
                    result.recommended_action = "check".into();
                }
            }
        }
    }
    if policy.enforce {
        result.action = result.recommended_action.clone();
    }
    result
}

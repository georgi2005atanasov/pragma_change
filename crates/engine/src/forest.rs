use crate::{EngineError, EngineResult, FeatureResult};
use pragma_domain::{Action, Decision, Forest, Thresholds};
/// Same leaf correction as sklearn (including its approximation for n > 2).
pub fn average_path_length(n: u32) -> f64 {
    match n {
        0 | 1 => 0.,
        2 => 1.,
        _ => {
            2. * ((n as f64 - 1.).ln() + 0.577_215_664_901_532_9) - 2. * (n as f64 - 1.) / n as f64
        }
    }
}
pub fn score(model: &Forest, vector: &[f64]) -> EngineResult<f64> {
    if vector.len() != model.feature_names.len()
        || vector
            .iter()
            .any(|x| !x.is_finite() || !(*x as f32).is_finite())
    {
        return Err(EngineError::Input("feature vector mismatch".into()));
    }
    let mut depth_sum = 0.;
    for tree in &model.trees {
        let mut index = 0;
        let mut depth = 0.;
        loop {
            let node = tree
                .get(index)
                .ok_or_else(|| EngineError::Input("invalid tree".into()))?;
            if node.left == -1 && node.right == -1 {
                depth_sum += depth + average_path_length(node.samples);
                break;
            }
            if depth > 64. || node.feature < 0 || node.feature as usize >= vector.len() {
                return Err(EngineError::Input("invalid tree".into()));
            }
            // sklearn converts inference input to float32 before comparing double thresholds.
            index = if (vector[node.feature as usize] as f32 as f64) <= node.threshold {
                node.left as usize
            } else {
                node.right as usize
            };
            depth += 1.;
        }
    }
    let denominator = model.trees.len() as f64 * average_path_length(model.max_samples);
    if denominator <= 0. {
        return Err(EngineError::Input("invalid forest".into()));
    }
    Ok(2f64.powf(-depth_sum / denominator))
}
pub fn decide(
    model: &Forest,
    thresholds: Thresholds,
    features: &FeatureResult,
) -> EngineResult<Decision> {
    if !features.ready {
        return Ok(Decision::check(
            model.pipeline_version_id,
            features.reason.clone(),
        ));
    }
    let risk = score(model, &features.vector)?;
    let action = if risk >= thresholds.block {
        Action::Block
    } else if risk >= thresholds.check {
        Action::Check
    } else {
        Action::Allow
    };
    Ok(Decision {
        action,
        risk_score: Some(risk),
        reason: "model_score".into(),
        pipeline_version_id: model.pipeline_version_id,
        model_version_id: Some(model.id),
    })
}

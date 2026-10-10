use crate::{EngineError, EngineResult};
use pragma_domain::{Operation, Pipeline, RequestEvent, Source};
use serde_json::Value;
use std::collections::BTreeMap;
fn scalar(v: &Value) -> bool {
    matches!(v, Value::Bool(_) | Value::Number(_) | Value::String(_)) && v.to_string().len() <= 4096
}
pub(crate) fn numeric(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|v: &f64| v.is_finite())
}
pub fn extract(p: &Pipeline, event: &RequestEvent) -> EngineResult<BTreeMap<String, Value>> {
    let route = p
        .routes()
        .find(|route| route.endpoint_id == event.endpoint_id && route.method == event.method)
        .ok_or_else(|| EngineError::Input("endpoint mismatch".into()))?;
    let params = route
        .path_params(&event.path)
        .ok_or_else(|| EngineError::Input("path mismatch".into()))?;
    if p.needs_body()
        && (event.body_size > p.limits.max_body_bytes as u64
            || event
                .body
                .as_ref()
                .is_some_and(|b| b.to_string().len() > p.limits.max_body_bytes))
    {
        return Err(EngineError::Input("body inspection limit".into()));
    }
    let mut out = BTreeMap::new();
    for e in &p.extractors {
        let value = match &e.source {
            Source::Header { name } => {
                let mut found = event
                    .headers
                    .iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case(name));
                let v = found.next().map(|(_, v)| Value::String(v.clone()));
                if found.next().is_some() {
                    return Err(EngineError::Input(format!("duplicate header {name}")));
                }
                v
            }
            Source::JsonPointer { pointer } => event
                .body
                .as_ref()
                .and_then(|b| b.pointer(pointer))
                .cloned(),
            Source::QueryParameter { name } => event.query.get(name).cloned().map(Value::String),
            Source::PathParameter { name } => params.get(name).cloned().map(Value::String),
            Source::ClientIp => event
                .client_ip
                .parse::<std::net::IpAddr>()
                .ok()
                .map(|ip| Value::String(ip.to_string())),
        }
        .filter(scalar)
        .ok_or_else(|| EngineError::Input(e.name.clone()))?;
        out.insert(e.name.clone(), value);
    }
    for f in &p.features {
        if f.operation.needs_source()
            && f.operation != Operation::DistinctCount
            && numeric(&out[f.source.as_ref().unwrap()]).is_none()
        {
            return Err(EngineError::Input(f.name.clone()));
        }
    }
    Ok(out)
}

use crate::data::{self, Import};
use pragma_behavior_contract::recording::{RecordingConfig, RecordingManifest};
use pragma_domain::Pipeline;
use serde_json::json;
use uuid::Uuid;
#[test]
fn sdk_request_recordings_replay_through_the_existing_feature_engine() {
    let p: Pipeline =
        serde_json::from_str(include_str!("../../../../examples/pipeline.json")).unwrap();
    let config:RecordingConfig=serde_json::from_value(json!({"format":"request_events","pipeline_version_id":p.id,"endpoint_id":p.endpoint_id,"schema_hash":p.schema_hash(),"allowed_headers":["X-Client-ID"],"allowed_body_fields":["amount"]})).unwrap();
    let content=(0..40).map(|i|{
        let request=json!({"endpoint_id":p.endpoint_id,"timestamp_ms":i*100,"method":"POST","path":"/orders","headers":{"X-Client-ID":"user","Authorization":"private"},"body":{"amount":i,"password":"private"},"body_size":900});
        config.sanitize(request).unwrap().to_string()+"\n"
    }).collect::<String>();
    let manifest:RecordingManifest=serde_json::from_value(json!({"format_version":1,"recording_id":Uuid::new_v4(),"config":config,"pipeline_version_id":p.id,"schema_hash":p.schema_hash(),"row_count":40,"byte_count":content.len(),"content_sha256":crate::hash::hash(content.as_bytes()),"finalized":true})).unwrap();
    let mut input:Import=serde_json::from_value(json!({"pipeline_version_id":p.id,"name":"SDK","format":"request_events","schema_hash":p.schema_hash(),"content":content,"recording_manifest":manifest})).unwrap();
    let prepared = data::prepare(&p, &input, &[0; 32]).unwrap();
    assert!(prepared.errors.is_empty());
    assert_eq!(prepared.source_rows, 40);
    assert_eq!(prepared.vectors.len(), 30);
    assert!(
        prepared.vectors.iter().all(|v| v[2] == 900.),
        "body_size is measured before privacy filtering"
    );
    input.content.push(' ');
    assert!(data::prepare(&p, &input, &[0; 32]).is_err());
    input.content.pop();
    input
        .recording_manifest
        .as_mut()
        .unwrap()
        .config
        .endpoint_id = Some(Uuid::new_v4());
    assert!(data::prepare(&p, &input, &[0; 32]).is_err());
    input.recording_manifest = None;
    assert!(
        data::prepare(&p, &input, &[0; 32]).is_ok(),
        "legacy JSONL imports remain supported"
    );
}

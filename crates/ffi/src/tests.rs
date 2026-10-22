use super::*;
use pragma_engine::MemoryStore;
use serde_json::json;

type SerializedStore = BTreeMap<String, Vec<u8>>;

unsafe extern "C" fn get(ctx: *mut c_void, key: PcSlice, out: *mut PcSlice) -> i32 {
    let store = unsafe { &*(ctx as *const SerializedStore) };
    match store.get(&unsafe { text(key) }.unwrap()) {
        Some(value) => {
            unsafe { *out = borrowed(value) };
            1
        }
        None => 0,
    }
}

unsafe extern "C" fn put(ctx: *mut c_void, key: PcSlice, value: PcSlice, _maximum: usize) -> i32 {
    let store = unsafe { &mut *(ctx as *mut SerializedStore) };
    store.insert(
        unsafe { text(key) }.unwrap(),
        unsafe { bytes(value) }.unwrap().to_vec(),
    );
    0
}

#[test]
fn guard_only_release_decides_without_a_model_route() {
    let release = Release {
        format_version: 2,
        release_id: uuid::Uuid::new_v4(),
        deployments: vec![],
        ingress_policies: vec![IngressPolicy {
            name: "upload_guard".into(),
            max_body_bytes: 1024,
            max_multipart_parts: 2,
            max_headers_per_part: 2,
            max_header_bytes_per_part: 256,
            max_field_nesting: 2,
            max_file_parts: 1,
        }],
    };
    let bytes = serde_json::to_vec(&release).unwrap();
    let secret = [7; 32];
    let mut error = [0; 256];
    let runtime =
        unsafe { pc_runtime_new(borrowed(&bytes), borrowed(&secret), error.as_mut_ptr()) };
    assert!(!runtime.is_null(), "{}", String::from_utf8_lossy(&error));
    assert_eq!(unsafe { pc_ingress_policy_count(runtime) }, 1);
    let mut maximum = 0;
    let guard = unsafe { pc_guard_policy(runtime, borrowed(b"upload_guard"), &mut maximum) };
    assert_eq!((guard, maximum), (0, 1024));
    let body = b"--x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x\"\r\n\r\na\r\n--x--\r\n";
    let mut decision = PcGuardDecision {
        action: 0,
        has_multipart_metrics: 0,
        reason: [0; 128],
        multipart_parts: 0,
        multipart_file_parts: 0,
        multipart_max_part_headers: 0,
        multipart_max_field_nesting: 0,
        multipart_total_header_bytes: 0,
        multipart_empty_file_parts: 0,
    };
    assert_eq!(
        unsafe {
            pc_guard_decide(
                runtime,
                guard,
                borrowed(b"multipart/form-data; boundary=x"),
                borrowed(body),
                &mut decision,
            )
        },
        0
    );
    assert_eq!(
        (
            decision.action,
            decision.has_multipart_metrics,
            decision.multipart_parts,
            decision.multipart_file_parts
        ),
        (0, 1, 1, 1)
    );
    let stream = unsafe {
        pc_guard_stream_new(runtime, guard, borrowed(b"multipart/form-data; boundary=x"))
    };
    assert!(!stream.is_null());
    for chunk in body.chunks(3) {
        assert_eq!(unsafe { pc_guard_stream_feed(stream, borrowed(chunk)) }, 0);
    }
    let mut streamed = decision;
    assert_eq!(unsafe { pc_guard_stream_finish(stream, &mut streamed) }, 0);
    assert_eq!(
        streamed.multipart_total_header_bytes,
        decision.multipart_total_header_bytes
    );
    assert_eq!(
        streamed.multipart_empty_file_parts,
        decision.multipart_empty_file_parts
    );
    unsafe { pc_guard_stream_free(stream) };
    assert_eq!(
        unsafe {
            pc_guard_decide(
                runtime,
                guard,
                borrowed(b"multipart/form-data; boundary=x"),
                borrowed(b"X"),
                &mut decision,
            )
        },
        0
    );
    assert_eq!(decision.action, 2);
    assert_eq!(&decision.reason[..19], b"malformed_multipart");
    unsafe { pc_runtime_free(runtime) };
}

#[test]
fn native_abi_matches_replay_for_all_features_and_rejects_ambiguous_inputs() {
    let mut pipeline: Pipeline =
        serde_json::from_str(include_str!("../../../examples/pipeline.json")).unwrap();
    pipeline.path_template = "/orders/{id}".into();
    pipeline.extractors.extend([
        Extractor {
            name: "query_amount".into(),
            source: Source::QueryParameter {
                name: "value".into(),
            },
        },
        Extractor {
            name: "route_id".into(),
            source: Source::PathParameter { name: "id".into() },
        },
        Extractor {
            name: "ip".into(),
            source: Source::ClientIp,
        },
    ]);
    pipeline.partitions[0]
        .components
        .extend(["route_id".into(), "ip".into()]);
    pipeline.features.clear();
    for op in [
        Operation::Count,
        Operation::Rate,
        Operation::Sum,
        Operation::Mean,
        Operation::Min,
        Operation::Max,
        Operation::Stddev,
        Operation::DistinctCount,
        Operation::InterarrivalMean,
        Operation::InterarrivalStddev,
    ] {
        pipeline.features.push(Feature {
            name: format!("f{}", pipeline.features.len()),
            operation: op,
            source: op.needs_source().then(|| "amount".into()),
            partition: Some("by_client".into()),
            window: Some("last_second".into()),
        });
    }
    pipeline.features.extend([
        Feature {
            name: "current_query".into(),
            operation: Operation::CurrentValue,
            source: Some("query_amount".into()),
            partition: None,
            window: None,
        },
        Feature {
            name: "body_bytes".into(),
            operation: Operation::BodySize,
            source: None,
            partition: None,
            window: None,
        },
    ]);
    let model = Forest {
        format_version: 1,
        id: ModelId(uuid::Uuid::new_v4()),
        pipeline_version_id: pipeline.id,
        schema_hash: pipeline.schema_hash(),
        feature_names: pipeline.features.iter().map(|f| f.name.clone()).collect(),
        max_samples: 100,
        trees: vec![vec![TreeNode {
            left: -1,
            right: -1,
            feature: -2,
            threshold: -2.,
            samples: 100,
        }]],
    };
    let thresholds = Thresholds {
        check: 0.4,
        block: 0.8,
    };
    let release = Release {
        format_version: 1,
        release_id: uuid::Uuid::new_v4(),
        deployments: vec![Deployment {
            pipeline: pipeline.clone(),
            model: model.clone(),
            thresholds,
        }],
        ingress_policies: vec![],
    };
    release.validate().unwrap();
    let bytes = serde_json::to_vec(&release).unwrap();
    let secret = [7; 32];
    let mut error = [0; 256];
    let runtime =
        unsafe { pc_runtime_new(borrowed(&bytes), borrowed(&secret), error.as_mut_ptr()) };
    assert!(!runtime.is_null(), "{}", String::from_utf8_lossy(&error));
    let mut needs_body = 0;
    let mut maximum = 0;
    let index = unsafe {
        pc_route(
            runtime,
            borrowed(b"POST"),
            borrowed(b"/orders/42"),
            borrowed(b"api.example.test"),
            &mut needs_body,
            &mut maximum,
        )
    };
    assert_eq!(
        (index, needs_body, maximum),
        (0, 1, pipeline.limits.max_body_bytes)
    );
    let mut replay = MemoryStore::default();
    let mut serialized = SerializedStore::new();
    for (timestamp, client, amount) in [
        (1000, "a", 2),
        (1250, "b", 90),
        (1250, "a", 4),
        (1500, "a", 8),
        (2000, "a", 16),
        (2250, "b", 10),
        (3100, "a", 1),
    ] {
        let body = format!("{{ \"amount\": {amount} }}");
        let headers = [PcHeader {
            name: borrowed(b"x-CLIENT-id"),
            value: borrowed(client.as_bytes()),
        }];
        let raw = PcRequest {
            timestamp_ms: timestamp,
            method: borrowed(b"POST"),
            path: borrowed(b"/orders/42"),
            host: borrowed(b"api.example.test"),
            query: borrowed(b"value=2%2E5&unused=x&unused=y"),
            client_ip: borrowed(b"127.0.0.1"),
            body: borrowed(body.as_bytes()),
            body_size: body.len() as u64,
            headers: headers.as_ptr(),
            header_count: headers.len(),
            input_error: 0,
            has_multipart_metrics: 0,
            multipart_parts: 0,
            multipart_file_parts: 0,
            multipart_max_part_headers: 0,
            multipart_max_field_nesting: 0,
            multipart_total_header_bytes: 0,
            multipart_empty_file_parts: 0,
        };
        let event = RequestEvent {
            timestamp_ms: timestamp,
            endpoint_id: pipeline.endpoint_id,
            method: "POST".into(),
            path: "/orders/42".into(),
            client_ip: "127.0.0.1".into(),
            headers: BTreeMap::from([("X-Client-ID".into(), client.into())]),
            query: BTreeMap::from([("value".into(), "2.5".into())]),
            body: Some(json!({"amount": amount})),
            body_size: body.len() as u64,
            multipart: None,
        };
        let expected = pragma_engine::features(&pipeline, &event, &secret, &mut replay).unwrap();
        let prepared = unsafe {
            pc_features(
                runtime,
                index,
                &raw,
                get,
                put,
                &mut serialized as *mut _ as *mut c_void,
            )
        };
        assert!(!prepared.is_null());
        let actual = unsafe { &*prepared }.features.as_ref().unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let expected_decision =
            output(pragma_engine::decide(&model, thresholds, &expected).unwrap());
        let mut decision = output(Decision::check(pipeline.id, "test"));
        assert_eq!(unsafe { pc_decide(runtime, prepared, &mut decision) }, 0);
        assert_eq!(decision.action, expected_decision.action);
        assert_eq!(decision.has_score, expected_decision.has_score);
        assert_eq!(decision.score, expected_decision.score);
        assert_eq!(decision.reason, expected_decision.reason);
        assert_eq!(
            decision.pipeline_version_id,
            expected_decision.pipeline_version_id
        );
        assert_eq!(
            decision.model_version_id,
            expected_decision.model_version_id
        );
        unsafe { pc_prepared_free(prepared) };
        for (duplicate, query, invalid) in [
            (true, b"value=2.5".as_slice(), 0),
            (false, b"value=2.5&value=3".as_slice(), 0),
            (false, b"value=2.5".as_slice(), 1),
        ] {
            let duplicates = [0, 1].map(|_| PcHeader {
                name: headers[0].name,
                value: headers[0].value,
            });
            let invalid_request = PcRequest {
                headers: duplicates.as_ptr(),
                header_count: if duplicate { 2 } else { 1 },
                query: borrowed(query),
                input_error: invalid,
                ..raw
            };
            let before = serialized.clone();
            let prepared = unsafe {
                pc_features(
                    runtime,
                    index,
                    &invalid_request,
                    get,
                    put,
                    &mut serialized as *mut _ as *mut c_void,
                )
            };
            assert_eq!(unsafe { pc_decide(runtime, prepared, &mut decision) }, 0);
            assert_eq!((decision.action, decision.has_score), (1, 0));
            assert_eq!(before, serialized, "invalid input must not advance history");
            unsafe { pc_prepared_free(prepared) };
        }
    }
    unsafe { pc_runtime_free(runtime) };
}

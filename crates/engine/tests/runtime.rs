use pragma_domain::*;
use pragma_engine::*;
use serde_json::json;
use std::collections::BTreeMap;
fn pipeline() -> Pipeline {
    serde_json::from_str(include_str!("../../../examples/pipeline.json")).unwrap()
}
fn event(p: &Pipeline, t: u64, client: &str, amount: f64) -> RequestEvent {
    RequestEvent {
        timestamp_ms: t,
        endpoint_id: p.endpoint_id,
        method: "POST".into(),
        path: "/orders".into(),
        headers: BTreeMap::from([("X-Client-ID".into(), client.into())]),
        query: BTreeMap::new(),
        client_ip: "127.0.0.1".into(),
        body: Some(json!({"amount":amount})),
        body_size: 32,
        multipart: None,
    }
}
#[test]
fn exact_window_and_partition_isolation() {
    let p = pipeline();
    p.validate().unwrap();
    let mut s = MemoryStore::default();
    let a = features(&p, &event(&p, 0, "a", 10.), &[7; 32], &mut s).unwrap();
    assert!(!a.ready);
    assert_eq!(a.vector, vec![1., 10., 32.]);
    features(&p, &event(&p, 500, "b", 900.), &[7; 32], &mut s).unwrap();
    features(&p, &event(&p, 500, "a", 20.), &[7; 32], &mut s).unwrap();
    let a = features(&p, &event(&p, 1000, "a", 40.), &[7; 32], &mut s).unwrap();
    assert!(a.ready);
    assert_eq!(a.vector, vec![2., 30., 32.]);
    assert_eq!(a.window_counts["by_client:last_second"], 2);
}
#[test]
fn composite_keys_are_typed_and_length_delimited() {
    let p = pipeline();
    let part = Partition {
        name: "pair".into(),
        components: vec!["a".into(), "b".into()],
    };
    let key = |a, b| {
        partition_key(
            &p,
            &part,
            &BTreeMap::from([("a".into(), a), ("b".into(), b)]),
            &[1; 32],
        )
        .unwrap()
    };
    assert_ne!(key(json!("ab"), json!("c")), key(json!("a"), json!("bc")));
    assert_ne!(key(json!(1), json!("x")), key(json!("1"), json!("x")));
}
#[test]
fn missing_input_never_updates_history() {
    let p = pipeline();
    let mut s = MemoryStore::default();
    let mut e = event(&p, 0, "a", 1.);
    e.body = None;
    assert!(features(&p, &e, &[0; 32], &mut s).is_err());
    assert!(s.partitions.is_empty());
}
#[test]
fn duplicates_and_out_of_order_fail() {
    let p = pipeline();
    let mut s = MemoryStore::default();
    let e = event(&p, 100, "a", 1.);
    features(&p, &e, &[0; 32], &mut s).unwrap();
    assert!(matches!(
        features(&p, &event(&p, 99, "a", 1.), &[0; 32], &mut s),
        Err(EngineError::OutOfOrder)
    ));
    let mut e = e;
    e.headers.insert("x-client-id".into(), "other".into());
    assert!(extract(&p, &e).is_err());
}
#[test]
fn route_overlap_and_parameters() {
    let mut p = pipeline();
    p.path_template = "/orders/{id}".into();
    assert_eq!(p.path_params("/orders/42").unwrap()["id"], "42");
    assert!(p.path_params("/orders/").is_none());
    let mut b = p.clone();
    b.path_template = "/orders/latest".into();
    assert!(p.overlaps(&b));
    b.method = "GET".into();
    assert!(!p.overlaps(&b));
}
#[test]
fn capacity_eviction_and_truncation_warm_up() {
    let mut p = pipeline();
    p.limits.max_partitions = 1;
    p.limits.max_events_per_partition = 2;
    let mut s = MemoryStore::default();
    for t in [0, 100, 200] {
        let f = features(&p, &event(&p, t, "a", 1.), &[0; 32], &mut s).unwrap();
        assert!(!f.ready);
    }
    features(&p, &event(&p, 1000, "b", 1.), &[0; 32], &mut s).unwrap();
    assert_eq!(s.partitions.len(), 1);
    assert!(
        !features(&p, &event(&p, 1100, "a", 1.), &[0; 32], &mut s)
            .unwrap()
            .ready
    );
}
#[test]
fn all_aggregates_and_order() {
    let mut p = pipeline();
    p.features = vec![];
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
        p.features.push(Feature {
            name: format!("f{}", p.features.len()),
            operation: op,
            source: op.needs_source().then(|| "amount".into()),
            partition: Some("by_client".into()),
            window: Some("last_second".into()),
        });
    }
    let mut s = MemoryStore::default();
    features(&p, &event(&p, 1000, "a", 2.), &[0; 32], &mut s).unwrap();
    let f = features(&p, &event(&p, 1500, "a", 4.), &[0; 32], &mut s).unwrap();
    assert_eq!(f.vector, vec![2., 2., 6., 3., 2., 4., 1., 2., 500., 0.]);
}
#[test]
fn corrupt_models_and_configuration_drift_are_rejected() {
    let p = pipeline();
    let mut m = Forest {
        format_version: 1,
        id: ModelId(uuid::Uuid::new_v4()),
        pipeline_version_id: p.id,
        schema_hash: p.schema_hash(),
        feature_names: p.features.iter().map(|f| f.name.clone()).collect(),
        max_samples: 2,
        trees: vec![vec![TreeNode {
            left: -1,
            right: -1,
            feature: -2,
            threshold: -2.,
            samples: 2,
        }]],
    };
    m.validate(&p).unwrap();
    m.trees[0][0].left = 0;
    m.trees[0][0].right = 0;
    m.trees[0][0].feature = 0;
    assert!(m.validate(&p).is_err());
    m.trees[0][0].left = -1;
    m.trees[0][0].right = -1;
    let mut p = p;
    p.windows[0].duration_ms = 2000;
    assert!(m.validate(&p).is_err());
}
#[test]
fn thresholds_and_float32_boundary() {
    assert!(
        Thresholds {
            check: 0.5,
            block: 0.5
        }
        .validate()
        .is_err()
    );
    let p = pipeline();
    let m = Forest {
        format_version: 1,
        id: ModelId(uuid::Uuid::new_v4()),
        pipeline_version_id: p.id,
        schema_hash: p.schema_hash(),
        feature_names: p.features.iter().map(|f| f.name.clone()).collect(),
        max_samples: 3,
        trees: vec![vec![
            TreeNode {
                left: 1,
                right: 2,
                feature: 0,
                threshold: 1.,
                samples: 3,
            },
            TreeNode {
                left: -1,
                right: -1,
                feature: -2,
                threshold: -2.,
                samples: 1,
            },
            TreeNode {
                left: -1,
                right: -1,
                feature: -2,
                threshold: -2.,
                samples: 2,
            },
        ]],
    };
    m.validate(&p).unwrap();
    assert_eq!(
        score(&m, &[1. + 1e-9, 0., 0.]).unwrap(),
        score(&m, &[1., 0., 0.]).unwrap()
    );
    assert!(score(&m, &[1., 0., 0.]).unwrap() > score(&m, &[2., 0., 0.]).unwrap());
}
#[test]
fn deterministic_replay() {
    let p = pipeline();
    let replay = || {
        let mut s = MemoryStore::default();
        (0..30)
            .map(|i| {
                serde_json::to_string(
                    &features(&p, &event(&p, i * 100, "a", i as f64), &[8; 32], &mut s).unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(replay(), replay());
}

#[test]
fn pipeline_cardinality_limits_do_not_evict_other_pipelines() {
    let a = pipeline();
    let mut b = a.clone();
    b.id = PipelineId(uuid::Uuid::new_v4());
    b.limits.max_partitions = 1;
    let mut store = MemoryStore::default();
    let first = features(&a, &event(&a, 0, "a", 1.), &[0; 32], &mut store).unwrap();
    features(&b, &event(&b, 0, "b", 1.), &[0; 32], &mut store).unwrap();
    features(&b, &event(&b, 0, "c", 1.), &[0; 32], &mut store).unwrap();
    assert!(
        store
            .partitions
            .contains_key(&first.partition_keys["by_client"])
    );
    assert_eq!(store.partitions.len(), 2);
}

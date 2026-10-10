use pragma_domain::*;
use pragma_engine::{MemoryStore, features};
use serde_json::json;
use uuid::Uuid;

fn pipeline() -> Pipeline {
    let mut p: Pipeline =
        serde_json::from_str(include_str!("../../../examples/pipeline.json")).unwrap();
    p.additional_endpoints.push(Endpoint {
        endpoint_id: Uuid::new_v4(),
        api_host: p.api_host.clone(),
        method: "PUT".into(),
        path_template: "/payments/{payment_id}".into(),
    });
    p
}

fn event(p: &Pipeline, additional: bool, t: u64, amount: f64) -> RequestEvent {
    let route = p.routes().nth(usize::from(additional)).unwrap();
    serde_json::from_value(json!({
        "timestamp_ms": t, "endpoint_id": route.endpoint_id, "method": route.method,
        "path": if additional { "/payments/123" } else { "/orders" },
        "headers": {"X-Client-ID": "account-a"},
        "body": {"amount": amount, "currency": "EUR", "description": "Unselected field"},
        "body_size": 100
    }))
    .unwrap()
}

#[test]
fn grouped_endpoints_share_history_and_one_forest() {
    let p = pipeline();
    p.validate().unwrap();
    let mut store = MemoryStore::default();
    let first = features(&p, &event(&p, false, 0, 10.), &[0; 32], &mut store).unwrap();
    features(&p, &event(&p, false, 500, 20.), &[0; 32], &mut store).unwrap();
    let second = features(&p, &event(&p, true, 1000, 40.), &[0; 32], &mut store).unwrap();
    assert_eq!(first.partition_keys, second.partition_keys);
    assert!(second.ready);
    assert_eq!(second.vector, vec![2., 30., 100.]);
    assert_eq!(store.partitions.len(), 1);
    let deployment = deployment(p);
    let decision =
        pragma_engine::decide(&deployment.model, deployment.thresholds, &second).unwrap();
    assert_eq!(decision.model_version_id, Some(deployment.model.id));
    let release = Release {
        format_version: 1,
        release_id: Uuid::new_v4(),
        deployments: vec![deployment],
    };
    release.validate().unwrap();
    assert_eq!(release.deployments.len(), 1);
    assert_eq!(release.deployments[0].pipeline.routes().count(), 2);
}

#[test]
fn separate_histories_have_independent_warmup_and_counts() {
    let mut p = pipeline();
    p.history_scope = HistoryScope::PerEndpoint;
    let mut store = MemoryStore::default();
    let a = features(&p, &event(&p, false, 0, 10.), &[0; 32], &mut store).unwrap();
    let b = features(&p, &event(&p, true, 1000, 20.), &[0; 32], &mut store).unwrap();
    assert_ne!(a.partition_keys, b.partition_keys);
    assert!(!b.ready);
    let a = features(&p, &event(&p, false, 1100, 40.), &[0; 32], &mut store).unwrap();
    assert!(a.ready);
    assert_eq!(a.vector, vec![1., 40., 100.]);
    assert_eq!(store.partitions.len(), 2);
}

#[test]
fn unselected_routes_wrong_methods_and_missing_fields_do_not_update_history() {
    let p = pipeline();
    let mut store = MemoryStore::default();
    for change in 0..4 {
        let mut e = event(&p, true, 0, 10.);
        match change {
            0 => e.endpoint_id = Uuid::new_v4(),
            1 => e.method = "POST".into(),
            2 => e.path = "/orders".into(),
            _ => e.body = Some(json!({"currency": "EUR"})),
        }
        assert!(features(&p, &e, &[0; 32], &mut store).is_err());
        assert!(store.partitions.is_empty());
    }
}

fn deployment(pipeline: Pipeline) -> Deployment {
    Deployment {
        model: Forest {
            format_version: 1,
            id: ModelId(Uuid::new_v4()),
            pipeline_version_id: pipeline.id,
            schema_hash: pipeline.schema_hash(),
            feature_names: pipeline.features.iter().map(|f| f.name.clone()).collect(),
            max_samples: 2,
            trees: vec![vec![TreeNode {
                left: -1,
                right: -1,
                feature: -2,
                threshold: -2.,
                samples: 2,
            }]],
        },
        pipeline,
        thresholds: Thresholds {
            check: 0.8,
            block: 0.9,
        },
    }
}

#[test]
fn groups_reject_duplicate_routes_and_release_conflicts_on_any_member() {
    let p = pipeline();
    let mut duplicate = p.clone();
    duplicate.additional_endpoints[0].endpoint_id = p.endpoint_id;
    assert!(duplicate.validate().is_err());
    duplicate.additional_endpoints[0].endpoint_id = Uuid::new_v4();
    duplicate.additional_endpoints[0].method = p.method.clone();
    duplicate.additional_endpoints[0].path_template = p.path_template.clone();
    assert!(duplicate.validate().is_err());

    let mut conflicting = p.clone();
    conflicting.id = PipelineId(Uuid::new_v4());
    conflicting.endpoint_id = Uuid::new_v4();
    conflicting.path_template = "/unrelated".into();
    let release = Release {
        format_version: 1,
        release_id: Uuid::new_v4(),
        deployments: vec![deployment(p), deployment(conflicting)],
    };
    assert!(release.validate().is_err());
}

#[test]
fn path_extractors_must_exist_in_every_group_member_and_use_the_matching_route() {
    let mut p = pipeline();
    p.extractors.push(Extractor {
        name: "payment".into(),
        source: Source::PathParameter {
            name: "payment_id".into(),
        },
    });
    assert!(p.validate().is_err());
    p.path_template = "/orders/{payment_id}".into();
    p.validate().unwrap();
    assert_eq!(
        pragma_engine::extract(&p, &event(&p, true, 0, 1.)).unwrap()["payment"],
        json!("123")
    );
}

#[test]
fn group_membership_and_history_changes_invalidate_model_compatibility() {
    let p = pipeline();
    let model = deployment(p.clone()).model;
    let mut changed = p.clone();
    changed.additional_endpoints.clear();
    assert!(model.validate(&changed).is_err());
    changed = p;
    changed.history_scope = HistoryScope::PerEndpoint;
    assert!(model.validate(&changed).is_err());
}

#[test]
fn aggregate_features_do_not_encode_endpoint_sequence() {
    let mut p = pipeline();
    for operation in [Operation::InterarrivalMean, Operation::InterarrivalStddev] {
        p.features.push(Feature {
            name: format!("timing_{}", p.features.len()),
            operation,
            source: None,
            partition: Some("by_client".into()),
            window: Some("last_second".into()),
        });
    }
    let replay = |sequence: [bool; 3]| {
        let mut store = MemoryStore::default();
        sequence
            .into_iter()
            .enumerate()
            .map(|(i, route)| {
                features(
                    &p,
                    &event(&p, route, i as u64 * 500, 10.),
                    &[0; 32],
                    &mut store,
                )
                .unwrap()
                .vector
            })
            .collect::<Vec<_>>()
    };
    // Swapping route identities while retaining timestamps and field values
    // cannot distinguish valid and invalid workflows with these features.
    assert_eq!(replay([false, true, false]), replay([true, false, false]));
}

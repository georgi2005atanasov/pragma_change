# Model scope assessment

Decision: new pipelines use one endpoint. Remove new endpoint grouping from the panel and reject it in the creation API. Keep immutable saved configurations and model/release files compatible. This decision concerns the current feature set; it does not imply cross-endpoint analysis is universally undesirable.

## What is useful across endpoints?

Cross-endpoint behavior can reveal excessive aggregate activity by one actor, resource enumeration across related routes, or suspicious business-flow timing. It is not limited to server execution time. OWASP API6 discusses excessive use of sensitive business flows and gives the example of an implausibly short interval between adding to a cart and completing a purchase. These are reasons to observe a business flow, not proof that pooling arbitrary routes into a single forest will detect it. [OWASP API6](https://api-security.owasp.org/editions/2023/en/0xa6-unrestricted-access-to-sensitive-business-flows/)

A common model can also be reasonable for genuinely equivalent operations, such as route aliases with equivalent features and baseline behavior. That suitability needs evidence from each endpoint, beyond matching field names or client/account partition keys.

## What this runtime can measure

- It can extract request fields, compute counts/rates, numeric aggregates, distinct scalar values, current values, body size, and request interarrival summaries.
- It runs before the upstream request, so it cannot know the current operation's execution duration or outcome. Response observations could inform later decisions only with additional instrumentation.
- The forest receives an ordered numeric vector, not endpoint identity or a sequence of operations. Saved shared groups can aggregate traffic, but the current feature vocabulary has no explicit route-transition or workflow-state feature. A regression test demonstrates that swapping endpoint identities while retaining timestamps and values leaves those aggregates unchanged.
- Keeping histories separate while sharing a model does not solve different endpoint baselines: the training distribution is still pooled.

Isolation Forest scores how readily a vector is isolated in the learned feature distribution. Consequently, an anomalous vector for one operation may resemble normal vectors from another operation. This is an inference from the algorithm and this implementation's missing endpoint context. [Scikit-learn outlier detection and Isolation Forest](https://scikit-learn.org/stable/modules/outlier_detection.html#isolation-forest)

## Reproducible counterexample

Run `services/trainer/.venv/bin/python scripts/evaluate-endpoint-sharing.py`.

One synthetic endpoint normally has about 3 requests/minute; another normally has about 60. The single numeric feature has exactly the same name and units. Each endpoint supplies 1,000 training, 250 calibration, and 250 separate test samples. Both comparisons use 100 trees, 256 samples/tree, the same seed, and a 99th-percentile normal calibration threshold. We then present busy-endpoint test vectors as bursts on the quiet endpoint.

With scikit-learn 1.9.1:

| Seed | Quiet-endpoint forest: bursts flagged | Pooled forest: bursts flagged |
|---|---:|---:|
| 42 | 250/250 | 4/250 |
| 137 | 250/250 | 5/250 |
| 2026 | 250/250 | 12/250 |

These are constructed cases, not measurements of production attack detection. There are no representative labeled production traces here. The example demonstrates a failure mode, not that shared models always perform worse. A pooled forest necessarily gives the same vector the same score regardless of which endpoint produced it; one group-wide threshold cannot recover that missing context.

## Implementation and future criteria

New pipelines select one endpoint, and the API returns a validation error for additional endpoints. Partitions still separate actors within the endpoint. Existing grouped versions retain their original hashes and behavior; a draft from one requires explicit selection of a single endpoint before saving. No existing records are rewritten or removed.

Reconsider sharing only with a concrete use case, compatible required-field semantics, coverage of every route in the training/evaluation data, and per-endpoint false-positive/detection measurements against separate baselines. Workflow detection additionally needs the relevant operation identities, transitions, timing intervals, outcomes, and application rules. Known invalid workflow steps should be enforced by application rules rather than relying on an anomaly score. Execution-time monitoring needs response instrumentation. These capabilities are not claimed or implemented by selecting several routes.

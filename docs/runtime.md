# Runtime and dataset contracts

## Pipeline identity

New pipeline creation accepts exactly one endpoint. The panel exposes a single endpoint selector and the API rejects nonempty `additional_endpoints`. Models use that endpoint's baseline; partition definitions separate clients/accounts within it. The [model scope assessment](model-scope.md) explains why unrestricted model sharing was withdrawn. Existing grouped configurations are still readable, replayable, trainable, and deployable without changing their immutable identity or history semantics. The multi-route fields below describe that compatibility format, not an available new-pipeline workflow.

A pipeline version binds its selected endpoint routes, extractor definitions, partition component order, windows, feature order, history scope, name, and state limits. Rust hashes its canonical struct serialization with SHA-256. Obtain that hash from the API/CLI; do not implement an independent JSON canonicalizer in a client.

The original `endpoint_id`, `api_host`, `method`, and `path_template` describe the first route. For new versions, the API resolves that route from its registered endpoint ID, ignoring client-supplied host/method/path values. In previously saved configurations, optional `additional_endpoints` entries have the same four fields; together these form a group of up to 200 routes using one model and one threshold pair. The first endpoint remains the database version-number anchor for backward compatibility; pipeline UUIDs identify versions uniquely. Optional `name` labels the pipeline. Routes cannot duplicate or overlap within a group or across deployments in the same release. Path-parameter extractors must exist in every selected route. Other configured extractors must be present at replay/runtime on every group member.

In the compatibility format, `history_scope` is `shared` (default) or `per_endpoint`. Shared history combines events with the same partition values across a saved group. Per-endpoint history maintains independent windows and warm-up per endpoint while using one forest. These settings are retained exactly when reading saved versions; there is no new grouping/history-scope control in the panel. A new single-endpoint version requires a new dataset and model. Absent/empty new fields and the default shared scope are omitted from canonical serialization to preserve legacy single-endpoint schema hashes. Grouped releases require the current native module; older modules reject unknown fields.

A trained forest binds `pipeline_version_id`, `schema_hash`, and ordered `feature_names`. Every load checks all three, format version, tree indices, feature indices, node reachability, finite thresholds, depth limits, and leaf sample counts. Model-only updates use the same pipeline ID and retain compatible state. Changed configurations get a new ID and separate history.

Field extraction accepts bounded scalar strings, numbers, and booleans. JSON null, objects, arrays, missing fields, non-finite numeric features, duplicate selected headers (case-insensitive), and duplicate selected query parameters are errors. Numeric features accept finite numbers or numeric strings. JSON Pointer implements RFC 6901 through serde_json. Header names are case-insensitive; other extracted strings are case-sensitive. IPs are normalized by Rust's IP parser; Nginx's resolved client address is used, not a caller-provided forwarding header.

Composite partition encoding includes the pipeline ID, partition name, and each ordered component serialized as a typed JSON scalar preceded by its byte length. With per-endpoint history, the fixed-width endpoint UUID also follows the pipeline UUID in the HMAC input. HMAC-SHA256 with a locally supplied 32-byte secret hides raw identities and avoids delimiter/type ambiguity. Returned keys prefix the digest with the public pipeline version ID so cardinality quotas stay separate. Quotas apply to the entire group, including when histories are separate. Secret rotation creates new partition keys and therefore starts warm-up again.

## State and feature semantics

Windows use processing time in production and the supplied timestamp during sorted offline replay. Nginx provides its monotonic millisecond clock, avoiding wall-clock corrections. Only elapsed times matter. Equal timestamps retain source order during dataset replay. Backward timestamps for an existing partition are rejected.

Interarrival features describe arrival gaps, not server execution duration. The engine inspects requests before forwarding them and does not collect response latency, response status, endpoint-transition features, or application workflow state. Aggregate features alone do not enforce business workflows.

The engine updates a partition exactly once per inspection, then evaluates all features over `(t-duration, t]`. Events exactly on the left boundary are excluded. Blocked requests also contribute to future behavior. A partition must have been observed for the longest window used by its features before its vector is ready. Quiet intervals count as observed time; there is no fabricated history before the first request.

| Operation | Semantics |
|---|---|
| `count` | Requests in the window |
| `rate` | Count divided by the complete window duration in seconds |
| `sum`, `mean`, `min`, `max` | Selected numeric field over window events |
| `stddev` | Population standard deviation (`ddof=0`) |
| `distinct_count` | Exact distinct typed scalar values |
| `interarrival_mean`, `interarrival_stddev` | Gaps in milliseconds between consecutive events *inside* the window; zero with fewer than two events |
| `current_value` | Selected current-request numeric field, without a window |
| `body_size` | Current request body size in bytes, without a window |

Only values used by aggregates are retained in state. Distinct counts are exact within the bounded retained history. No silent approximate aggregates are substituted.

The reference store is an in-process map for replay. The Nginx store uses a shared slab-backed red-black tree, an LRU queue, and a process-shared mutex. Rust receives serialized per-partition state through C callbacks, never pointers to another worker's heap. Parsing/state transitions are currently serialized by the shared zone lock; inference happens after unlocking. Benchmark your actual feature set before choosing a throughput target: this implementation prioritizes consistent bounded behavior over lock-free throughput claims.

`max_events_per_partition` truncation marks the history incomplete and restarts warm-up. Cardinality or slab-memory pressure evicts LRU partitions, which start warm-up on their next event. The shared zone bounds total memory. A serialization exceeding `max_partition_bytes` yields `check` with a capacity reason. Cardinality limits are scoped to each pipeline version. Slab memory is bounded for the entire zone; memory pressure can evict the globally least recently used partition.

Partial history is never presented as a scored normal request. A not-ready/error result is `check` with no score. The backend must implement the desired verification behavior.

## Request-event JSONL

One object per line:

```json
{"timestamp_ms":1000,"endpoint_id":"22222222-2222-4222-8222-222222222222","method":"POST","path":"/orders","headers":{"X-Client-ID":"client-a"},"query":{},"client_ip":"192.0.2.1","body":{"amount":42},"body_size":13}
```

The dataset identifies one pipeline version. Events may belong to any endpoint selected in its group; each row must carry the actual endpoint ID and matching method/path. Include representative traffic from all group members. The importer sorts by timestamp, preserving source order on ties, and follows the pipeline's shared or per-endpoint history scope. Events before warm-up completes are counted and excluded from the resulting training matrix. Invalid rows are reported with source line numbers; a dataset containing any error is not imported. The preview reports up to 100 row errors. Unselected fields inside `body`, `headers`, and `query` are accepted; unknown top-level event fields are rejected. Template responses include `request_events`, one example per endpoint, and retain `request_event` for the first endpoint for compatibility.

`body_size` must reflect the actual original body length; reconstructing a JSON object with different whitespace can change it. Use representative data with normal diversity and sufficient history. Request bodies in imported datasets are stored in the private artifact volume; production requests are not captured.

## Feature-vector CSV

```csv
request_count,amount_mean,body_bytes
8,42.5,32
6,38.0,29
```

The exact header names and order must match the saved pipeline. Every cell must be a finite number representable in float32. The separate import manifest supplies `pipeline_version_id` and `schema_hash`. Rows are treated as already-computed complete observations and retain file order. The system can validate their shape, not prove that your external aggregator used the correct window semantics.

The portal downloads a manifest and a format template and explains both choices. Imports require at least 20 usable vectors; 100,000 source rows and a 16 MiB HTTP request are the upper bounds. The JSON upload envelope counts toward the HTTP limit, so the file picker limits source files to 15 MiB.

## Training and scoring

Rust's PostgreSQL-backed queue claims jobs with `FOR UPDATE SKIP LOCKED`. A claim has a six-minute lease; the HTTP training call times out after five minutes. Attempts are fenced when committing results. Failed jobs retry after an attempt-scaled delay, up to three attempts. Expired final leases become failed instead of staying stuck. An administrator can explicitly retry failed jobs.

Python always trains with all features, no bootstrap, `contamination=auto`, an explicit seed, and one training worker. Configurable tree count and subsample size are bounded. `max_samples` is capped by the training subset. Retrying the same job/data/parameters is deterministic; the job UUID is also its model artifact UUID. Raw pickle uploads and arbitrary code compilation are unsupported.

The last 20% of matrix rows are held out. Rust verifies exported predictions on holdout examples and float32 values immediately surrounding tree split thresholds with absolute tolerance `1e-7`. Evaluation records the actual sklearn version and training parameters. Data-only exports include child indices, selected feature, threshold, leaf sample count, and forest subsample size. Inputs are cast to float32 before comparing against double thresholds, matching sklearn.

## Operational boundaries

The endpoint-forest workflow does not capture production traffic. The optional behavior service and SDKs provide separately scoped application-event collection; see [expansion contracts](expansion.md). PostgreSQL contains configuration, metadata, sessions, jobs, and audits; the artifact volume contains imported source files, validated matrices, and models. Back up both together. Imported endpoint artifacts retain their existing manual retention policy. Behavior-service retention and explicit purge are separate.

The native adapter only applies to explicitly protected locations and matching host/method/path definitions. It runs in the pre-access phase so `satisfy any` cannot override a block. Configure the normal Nginx authentication/rate-limit modules separately. No JavaScript challenges or external verification callbacks are implemented.

## Apache adapter

The [Apache native adapter](apache.md) uses this same release, extraction, feature, partition, and decision contract. Its APR shared-memory store spans child processes and threads. It uses monotonic processing time, keeps state for model-only graceful reloads, and starts cold on full restart. Apache-specific request normalization, header ambiguity handling, body replay, and installation boundaries are documented in the adapter guide.

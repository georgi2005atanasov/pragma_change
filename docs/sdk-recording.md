# Record now, upload and train later

`DatasetRecorder` writes **files on the application machine** without contacting PragmaChange. Keep the existing streaming client's `record`/`evaluate` methods for live collection and checks. The two paths are independent; calling one does not silently enable the other.

| Recording | Binding | Upload destination | Training |
|---|---|---|---|
| `request_events` | Saved pipeline version, schema hash, endpoint | Datasets | Existing Isolation Forest |
| `behavior_events` | Registered behavior application | Behavior → Import an offline recording | Markov sequences, personal/peer estimates, transition timing |

A recorder creates a new directory containing `records.jsonl` and `manifest.json`. Each successful record call appends and synchronizes one line to disk. Closing finalizes a manifest with a stable recording ID, counts, SHA-256, format and field-selection settings. The JSONL contains records, **not precomputed feature vectors or a generated feature pipeline**. Rust still derives request partitions/windows/features during import. Behavior snapshots preserve event IDs, timestamps and workflow boundaries.

Choose partition keys, windows, schema and features in the portal (or review an Assisted setup proposal) before collecting a request dataset. An arbitrary number of records is not a guarantee of sufficient or representative training data.

These are optional reference implementations, not published SDK products. Clone the [examples repository](https://github.com/georgi2005atanasov/pragma_change_sdks) alongside this checkout as `pragma_change_sdks` to use the relative paths below. Access is currently restricted. You can instead implement the documented formats yourself; plain request JSONL and feature CSV imports remain supported without a recording manifest.

## Request recorder configuration

In **Datasets**, select the saved pipeline and download its templates. Save `dataset-manifest.json` and `request-events.jsonl` in your working directory. The following example creates a recorder configuration for the sample orders pipeline, which reads `X-Client-ID` and `/amount`. For another pipeline, choose its actual required fields explicitly:

```bash
python3 - <<'PY'
import json
from pathlib import Path
manifest = json.loads(Path('dataset-manifest.json').read_text())
example = json.loads(Path('request-events.jsonl').read_text().splitlines()[0])
config = {
    'format': 'request_events',
    'pipeline_version_id': manifest['pipeline_version_id'],
    'schema_hash': manifest['schema_hash'],
    'endpoint_id': example['endpoint_id'],
    'allowed_headers': ['X-Client-ID'],
    'allowed_query': [],
    'allowed_body_fields': ['amount'],
    'include_client_ip': False,
}
Path('recording-config.json').write_text(json.dumps(config, indent=2))
PY
```

The pipeline version/hash must describe the actual saved schema. Never change the hash to disguise a schema mismatch. One request recorder targets one endpoint. Use multiple recorders for multiple endpoint models.

Select only necessary data. Headers are normalized to lowercase; query/body fields are exact names. This first recorder supports **top-level scalar body fields**. It rejects a selected nested object/array instead of writing its contents. Nested JSON-pointer extractors require an explicitly compatible collection approach; flattening changes the pipeline and must be reviewed before saving its version.

Supply `body_size` measured from the original request bytes **before filtering**. Supply the path without query/fragment. `client_ip` is omitted from collection unless explicitly enabled. Capture the HTTP values as seen at your enforcement boundary: application middleware may see transformed headers, bodies or paths. Dataset validity does not establish equivalence to Nginx-observed traffic.

## Python: a complete local practice recording

Run from the repository root after creating `recording-config.json` above. This creates synthetic example data solely for testing the import flow:

```bash
mkdir -p recordings
PYTHONPATH=../pragma_change_sdks/python python3 - <<'PY'
import json
from pathlib import Path
from pragma_change import DatasetRecorder
config = json.loads(Path('recording-config.json').read_text())
with DatasetRecorder('recordings/orders-python', config) as output:
    for index in range(40):
        output.record({
            'timestamp_ms': 1700000000000 + index * 100,
            'endpoint_id': config['endpoint_id'],
            'method': 'POST', 'path': '/orders',
            'headers': {'X-Client-ID': 'example-user'},
            'body': {'amount': 10 + index % 5},
            'body_size': 30,
        })
PY
```

For the example one-second window this produces 30 complete vectors after warm-up. In **Datasets**, choose `records.jsonl`, choose its **SDK recording manifest**, give the dataset a name, preview, and accept. Then train from that dataset. The server checks the exact file bytes and bindings and retains the recording manifest alongside the source. Legacy JSONL/CSV imports without a recording manifest remain supported.

## Node.js / TypeScript

Import from `@pragmachange/sdk` after installing the local package. From this repository root, the equivalent is:

```javascript
import { readFileSync } from 'node:fs';
import { DatasetRecorder } from './../pragma_change_sdks/node/index.js';
const config = JSON.parse(readFileSync('recording-config.json', 'utf8'));
const recorder = new DatasetRecorder('recordings/orders-node', config);
try {
  recorder.record({
    timestamp_ms: Date.now(), endpoint_id: config.endpoint_id,
    method: 'POST', path: '/orders', body_size: 30,
    headers: { 'X-Client-ID': 'example-user' }, body: { amount: 12 },
  });
} finally {
  recorder.close();
}
```

Use a `.mjs` file or an ES module project. `DatasetRecorder.record` writes synchronously; do not run high-volume disk synchronization on a latency-sensitive Node request loop. Put collection in an application-owned worker when necessary and define its own bounded queue/failure policy. The current recorder does not provide a durable background spool.

## C#

Reference `../pragma_change_sdks/csharp/PragmaChange.csproj`. The recorder takes the same JSON configuration and either a `JsonObject` request or a typed `BehaviorEvent`:

```csharp
using System.Text.Json.Nodes;
using PragmaChange;
var config = JsonNode.Parse(File.ReadAllText("recording-config.json"))!.AsObject();
using var recorder = new DatasetRecorder("recordings/orders-csharp", config);
recorder.Record(new JsonObject {
    ["timestamp_ms"] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
    ["endpoint_id"] = config["endpoint_id"]!.GetValue<string>(),
    ["method"] = "POST", ["path"] = "/orders", ["body_size"] = 30,
    ["headers"] = new JsonObject { ["X-Client-ID"] = "example-user" },
    ["body"] = new JsonObject { ["amount"] = 12 }
}); // Dispose finalizes manifest.json
```

The checked-in C# contract runner can also turn existing JSONL into a filtered recording:

```bash
dotnet run --project ../pragma_change_sdks/contract/csharp/Contract.csproj -- \
  --record recordings/orders-csharp recording-config.json request-events.jsonl
```

## Rust

The recorder is synchronous and needs no Tokio runtime; the HTTP client still does. Add `serde_json` to your application's dependencies when using this example:

```rust
use pragmachange::{DatasetRecorder, recording::RecordingConfig};
use serde_json::json;
let config: RecordingConfig = serde_json::from_slice(
    &std::fs::read("recording-config.json")?
)?;
let endpoint = config.endpoint_id.unwrap();
let mut recorder = DatasetRecorder::create("recordings/orders-rust", config)?;
recorder.record(json!({
    "timestamp_ms":1700000000000_u64, "endpoint_id":endpoint,
    "method":"POST", "path":"/orders", "body_size":30,
    "headers":{"X-Client-ID":"example-user"}, "body":{"amount":12}
}))?;
recorder.close()?; // mandatory: Drop does not finalize
```

Or use the runnable example from the repository root:

```bash
cargo run --manifest-path ../pragma_change_sdks/Cargo.toml -p pragmachange --example recording -- \
  recordings/orders-rust recording-config.json request-events.jsonl
cargo run --manifest-path ../pragma_change_sdks/Cargo.toml -p pragmachange --example recording -- --verify recordings/orders-rust
```

## Behavior recording and training practice

Create an application in **Behavior**, then replace the application ID below with the real ID. No ingestion token or running service is needed while recording locally. This generates ten small synthetic workflows that meet the current trainer's minimum split sizes:

```bash
mkdir -p recordings
export PRAGMA_APPLICATION_ID=YOUR_REGISTERED_APPLICATION_UUID
PYTHONPATH=../pragma_change_sdks/python python3 - <<'PY'
import os
from uuid import uuid4
from pragma_change import DatasetRecorder
config = {'format': 'behavior_events', 'application_id': os.environ['PRAGMA_APPLICATION_ID'],
          'allowed_attributes': ['duration_ms']}
with DatasetRecorder('recordings/behavior-python', config) as output:
    for workflow in range(10):
        for step in range(40):
            output.record({
                'version': 1, 'event_id': str(uuid4()),
                'application_id': config['application_id'],
                'actor': 'example-user', 'session_id': 'example-session',
                'workflow_id': f'workflow-{workflow}',
                'action': ['document.open', 'document.read'][step % 2],
                'timestamp_ms': 1700000000000 + workflow * 100000 + step * 100,
                'outcome': 'succeeded', 'cohort': 'readers',
                'sequence_start': step == 0, 'sequence_number': step,
                'attributes': {'duration_ms': 25},
            })
PY
```

In **Behavior → Import an offline recording**, select the matching application, `manifest.json` and `records.jsonl`. The server validates the complete upload before inserting one snapshot. It HMAC-pseudonymizes actor/session/workflow/resource IDs and applies the service's attribute allowlist too. Importing the identical finalized recording again returns that snapshot. A changed file/settings with the same recording ID is rejected. Duplicate event IDs inside a file are rejected rather than silently changing the training sample.

Select **Train sequence model**, wait for the job, and replay a separate representative snapshot. Imported history does not enter live sequence state. Offline correlations need not refer to an evaluation previously sent to the service; matching attempts included in the file are checked. Activation remains a separate reviewed operation, and new applications still default to shadow mode.

The synthetic example proves the plumbing only. For meaningful training, collect legitimate workflows and representative rare actions from trusted server-side identities. Keep concurrent workflows separate and record confirmed outcomes separately from attempts. The trainer requires at least five workflows, 100 usable training events and 20 events in each calibration/test split; chronology, missing boundaries and per-actor caps can reduce usable data. See [model behavior and limits](expansion.md#models-and-evidence).

To stream and record the same behavior event, construct its identity once:

```javascript
const event = client.event(authenticatedAction);
recorder.record(event);  // durable local copy
client.record(event);    // existing asynchronous stream, same event ID
```

Python uses `event = client.event(**authenticated_action)`, then `recorder.record(event)` and `client.record(**event)`. C#/Rust use the same event object/clone. Both paths can be enabled, but importing an offline recording always makes its own snapshot; it does not merge it with a snapshot of streamed events. Avoid training twice on overlapping copies.

## Generate compatible files yourself

No client library is required. For ordinary endpoint imports, write the request JSONL or feature CSV described by the saved pipeline templates and upload with its dataset binding. A dataset manifest identifies a pipeline; it is different from the checksummed **recording manifest** below.

For an offline recording, implement the same contract in your own language:

| Recording manifest field | Meaning |
| --- | --- |
| `format_version` | Integer `1`. |
| `recording_id` | A fresh UUID for this recording, retained on an identical retry. |
| `config` | The format, application or pipeline/endpoint binding, field allowlists, and bounds shown above. |
| `pipeline_version_id`, `schema_hash` | Equal the values in `config` for requests; `null` for behavior recordings. |
| `row_count` | Exact nonzero JSONL line count, within `config.max_records`. |
| `byte_count` | Exact UTF-8 file byte count, within `config.max_bytes`. |
| `content_sha256` | Lowercase SHA-256 hexadecimal digest of the complete JSONL bytes. |
| `finalized` | `true` only after the complete recording has been successfully closed. |

Write one valid JSON object per line and include the final newline. Do not include blank lines or reformat the file after hashing it. Request events carry the actual endpoint, timestamp, method, path, selected fields, and original body size. Behavior events carry the v1 application-event fields, fresh event IDs, and trustworthy workflow boundaries; see the behavior API contract. Unknown manifest fields are rejected. The server also validates each row and the target binding; a valid checksum alone does not establish a valid dataset.

The same hard limits apply to custom recordings: 4 MiB per file, 100,000 rows, and 16 KiB per row. Use explicit field allowlists and private local storage. Do not finalize an abandoned reference-recorder file by hand to bypass a failed write; create your own complete, validated export instead.

## Limits and privacy

- Defaults and hard maximums: 4 MiB/file, 100,000 rows, 16 KiB/row. You can lower `max_bytes` and `max_records`. A full recorder raises an error; close it and start a **new directory**. It never silently discards or overwrites records. There is no automatic rotation or multi-file snapshot merge yet; choose a complete representative capture that fits these limits.
- Selected body/attribute values are bounded scalars. Headers, query, body and attributes are empty by default; IP collection is off. The manifest preserves explicit allowlists. Required metadata, identities, path and workflow fields are still data you must choose responsibly; do not place secrets in action names or URL path segments.
- Local actor/session/workflow/resource strings remain as supplied. Supply stable pseudonymous IDs if local raw identities are unsuitable. Server HMAC pseudonymization occurs **on import/stream ingestion**, not magically on the application filesystem.
- Unix directories/files use owner-only permissions (0700/0600). On Windows, use a private directory with an appropriate ACL; Unix modes do not apply. One writer owns each new directory.
- Files survive successful synchronized appends. A failed write poisons the writer and prevents finalization. A crash/abandoned writer leaves `finalized: false`; import rejects it. This version has no automatic crash-recovery tool. Preserve unfinished data for deliberate recovery; start a new recording rather than editing a checksum or claiming completeness.
- Close/dispose explicitly at the capture boundary. Zero-row recordings cannot be imported. Keep the JSONL byte-for-byte: changing newlines, encoding or values invalidates its checksum. A checksum detects content changes; it is not a signature proving who collected the data.

# Remote operations and application behavior

The management stack, Nginx runtime, and application behavior service are separate processes with separate responsibilities. Nginx uses its last activated local release without contacting the portal. Training, provider requests, dataset preparation and module builds run off the Nginx host. Inspection still consumes CPU and bounded shared memory on the proxy; benchmark the actual workload.

Copyable install/start/stop/test commands are in [setup commands](setup-commands.md). Local SDK file output and upload examples are in [offline recording](sdk-recording.md).

## Remote Nginx setup

The first remote transport supports host-installed Nginx on Linux, using a dedicated SSH account. Nginx must handle HTTP; opaque TCP/TLS passthrough is not inspectable by this module.

1. On the management machine, generate a private credential key file containing 64 hexadecimal characters. Set `CREDENTIAL_KEY_FILE` to its absolute path and restrict its permissions to `0600`. Retain this key with backups: encrypted SSH and provider credentials cannot be recovered without it. Do not reuse `PARTITION_SECRET`.
2. On the proxy, install Python 3, OpenSSH server, sudo, OpenSSL and a compatible `pragma-cli`. Compile the module elsewhere against the exact target Nginx build and platform; the bundled 1.28.0 reference build is not universal.
3. Generate an Ed25519 artifact-signing key off-host. Install its public key on the proxy. Package a module with `scripts/remote/package-module.py`, providing the exact captured `nginx -V` stdout followed by stderr, target architecture, and signing key. The signed manifest binds the binary digest to the target build. The signing key must never be available to the proxy.
4. Provision an SSH account that has no unrelated sudo privileges. Run `scripts/remote/bootstrap.py` once as root, supplying `--account`, `--module-public-key`, and `--health-url`. Supply the actual `--nginx`, `--config`, `--prefix`, `--validator` and `--service` where defaults differ. Inspect the generated policy at `/etc/pragmachange-helper.json`. The configured health URL must be a safe read-only endpoint; configure its expected host/status directly in this root-owned policy if needed.
5. The bootstrap installs root-owned helper code and one no-argument sudo entry. Use an SSH authorized-key restriction such as `restrict` for the deployment key; the account must not be able to modify helper code, policy, Nginx configuration, or managed artifacts directly. Restrict interactive access separately according to your SSH policy.
6. Register the account in **Servers**, including its private SSH key and the independently verified server host public key (`ssh-ed25519 AAAA…`, without hostname/comment). The controller does not discover and silently trust host keys. Encrypted private keys are never returned by list APIs.
7. Inspect, stage the signed module, inspect again, select locations, and prepare an integration preview. Select that preview and an existing release, review the diff, then deploy.

Uploaded configuration analysis reports locations but cannot resolve the target’s includes, permissions, TLS files or loaded modules. Target inspection and validation are mandatory. Automatic integration supports a single top-level HTTP block and ordinary locations, including discovered include files. Conditional/nested or ambiguous structures require a manual reviewed integration. Only the necessary Pragma directives and managed include files are added.

Module files and model bundles are separate. Ordinary model releases validate and reload. Existing native-module upgrades use **Prepare module upgrade**, require explicit restart acknowledgement, and use the fixed service restart command in the host’s root-owned policy. Restart resets in-memory histories and may interrupt traffic: drain the host or schedule maintenance first. A failed ordinary reload never escalates to restart. Keep the correct Nginx service command in the policy; custom installations must update it before enabling module upgrades.

Deployment operations are journaled on both machines. A connection interruption produces `unknown`, not success. Reconciliation consults the host journal and restores a pre-change snapshot for interrupted activations rather than re-executing them. After recovery, inspect again before a new deployment. Configuration drift invalidates old previews. Serial rollouts stop at a failed or uncertain predecessor.

The helper’s status location is on a Unix socket inside its private managed directory. The native handler rejects TCP access and reports the release held in the worker’s memory. The helper also verifies the immutable module path mapped by that responding worker through Linux `/proc`. This distinguishes installed files from a loaded release. Successful verification checks a serving worker generation plus representative health probes; draining workers may still complete old requests after reload. The portal records the timestamp of verification, not a claim of continuous host monitoring.

`REMOTE_TRANSPORT` optionally selects the controller transport script path. The API container includes Python and OpenSSH; mount `CREDENTIAL_KEY_FILE` read-only into it and set the container path through a Compose override. Do not mount proxy configuration into the portal.

## Application behavior service

Run `pragma-behavior` on an application-side host with its own PostgreSQL database:

- `BEHAVIOR_DATABASE_URL`: dedicated database; its migrations are independent of management migrations.
- `BEHAVIOR_ADMIN_TOKEN`: distinct random credential of at least 32 characters.
- `BEHAVIOR_PARTITION_SECRET`: stable 64-character hexadecimal HMAC key.
- `BEHAVIOR_BIND_ADDR`: defaults to `127.0.0.1:8090`.

Set `BEHAVIOR_URL` and the matching `BEHAVIOR_ADMIN_TOKEN` on the management API to enable **Behavior**. `compose.behavior.yaml` is an optional local development overlay with a separate behavior database; a production deployment can run that stack on another machine. Use private authenticated transport or TLS termination for cross-host SDK requests; never expose the plaintext listener publicly.

Create an application and copy its one-time ingestion token. Each token is scoped to one application. Server-side authentication supplies actor, session and cohort identity; arbitrary client headers must not supply trusted identity. Application IDs and credentials are not browser instrumentation keys.

Optional [reference client sources](https://github.com/georgi2005atanasov/pragma_change_sdks), shown as sibling-checkout paths:

- Node.js/TypeScript: `../pragma_change_sdks/node`, package `@pragmachange/sdk`.
- Python: `../pragma_change_sdks/python`, package `pragmachange`, import `pragma_change`.
- C#: `../pragma_change_sdks/csharp`, package `PragmaChange` targeting .NET 8.
- Rust: `../pragma_change_sdks/rust`, crate `pragmachange`.

These are adaptable reference implementations, not finished SDK products. Packages are local sources and have not been published to registries. Each provides bounded asynchronous recording, at most five delivery attempts, retry-stable event IDs, collection counters, an explicit flush operation, and synchronous evaluation. Collection is best effort: queue overflow and terminal failures increment `dropped`; monitor it. Flush/close should run during application shutdown. Close drains for up to five seconds plus an in-flight request and counts any remaining events as dropped. The Rust client requires a Tokio runtime and depends on the small `pragma-behavior-contract` crate, not the training service. C# base URLs should end in `/`.

The v1 event contract requires `version`, `event_id`, `application_id`, `actor`, `action`, `session_id`, `timestamp_ms`, and `outcome`. Optional fields are `resource`, `workflow_id`, `correlation_id`, `cohort`, `sequence_start`, `sequence_number`, and scalar `attributes`. Action names are stable symbols such as `document.export`, not URLs containing resource IDs. See `behavior-openapi.json`.

Use a distinct workflow ID for independently concurrent journeys. Emit `sequence_start: true` only at a known boundary, with monotonically increasing sequence numbers when possible. Missing boundaries, backward time, and detected sequence gaps make evidence incomplete. History is bounded to 128 completed actions. Neither distributed wall clocks nor asynchronous delivery establish a global causal ordering by themselves.

Call `evaluate` **after ordinary authorization but before the side effect**. An `allow` does not itself authorize a resource or reserve business state. `check` requires application verification before proceeding; `block` stops the action. SDK transport failure returns `check`. Record the actual result afterward with a new event ID and `correlation_id` set to the evaluation’s event ID. The service validates its actor, workflow, action and resource against the attempt and permits one confirmed outcome. Standalone observed events may omit correlation. Retrying a delivery must preserve the event ID and content.

Middleware records response outcomes and duration. Node middleware must run after authentication; Python ASGI and C# callbacks resolve authenticated context. Rust provides an operation wrapper. Configure allowlisted attributes on both SDK and service; attributes are stripped by default. Actor/session/workflow/resource identities are HMAC-pseudonymized on ingestion. This is pseudonymization, not anonymization. No raw request bodies are automatically collected.

Each SDK also provides an explicit `DatasetRecorder` that writes private JSONL and a checksummed manifest locally, independent of streaming. Request recordings bind to a saved pipeline and import into Datasets; behavior recordings import into immutable snapshots through `POST /admin/apps/{id}/imports`. Offline uploads never alter live history. See [recording contracts and examples](sdk-recording.md).

Example Node integration:

```javascript
const pc = new PragmaChange({url: process.env.PRAGMA_URL,
  token: process.env.PRAGMA_TOKEN, applicationId: process.env.PRAGMA_APP});
const attempt = pc.event({actor: verifiedUser.id, session_id: verifiedSession.id,
  workflow_id: exportWorkflow.id, action: 'document.export', resource: document.id,
  outcome: 'attempted'});
const decision = await pc.evaluate(attempt);
// The application enforces check/block before executing its authorized operation.
// After a confirmed result:
pc.record({...attempt, event_id: crypto.randomUUID(),
  correlation_id: attempt.event_id, outcome: 'succeeded', timestamp_ms: Date.now()});
```

## Models and evidence

Snapshots are immutable copies of accepted, non-quarantined events, with content hashes. Sequence training uses bounded order-three Markov contexts, smoothing, backoff, role/cohort priors, and personal estimates shrunk toward peers. At most 1,000 events per actor contribute to training. Vocabulary is limited to 512 action names.

Whole workflows are ordered chronologically into 60% training, 20% calibration, and 20% test groups; workflows crossing a boundary are purged. Training requires at least five workflows, 100 complete training events, and 20 events in each later split. Reported holdout flag rates are not attack accuracy. Transition timing requires at least 20 observations for a transition. The sustained alert-rate monitor requests review when more than 25 of the latest 100 evaluations have unusual transitions; it is an operational heuristic, not ADWIN or proof of an attack.

New applications start in shadow mode. Sequence anomalies recommend `check`; explicit prerequisite rules may recommend `block`. Personal and peer evidence remain visible, and numeric forest scores remain a separate endpoint capability. Activating a model resets live sequence history so surprise values from different generations are not mixed. Replay operates on isolated state. Enforce only after representative labeled evaluation and application handling of all three actions.

Raw events, decisions and live state expire according to the policy’s 1–365 day retention. Snapshots and models deliberately remain until explicit application-data deletion, retaining reproducibility. The admin purge endpoint removes snapshots, jobs, models, decisions, events and live history, and deactivates the model. Quarantining an event excludes it from new snapshots; retrain explicitly if an existing model was contaminated.

## Assisted setup

Configure Claude in **Assisted setup** with a model available to your account. The provider key uses the same externally keyed credential vault as SSH credentials. Each request sends only the explicitly reviewed description, registered endpoint schemas, and a configuration example. It does not automatically send captured events, SSH details, database contents, or raw requests.

Each job makes one provider call with a 64,000-byte serialized input-context cap and a 4,096 output-token cap. A rolling 24-hour request limit bounds call volume; this is not an exact currency budget. Provider failures do not trigger automatic paid retries. Configure any currency budget in the provider account as well.

Claude generates a structured data-only draft, not just a prose report: pipeline extractors, partition definitions, window definitions, ordered features, limits, and optional behavior policy/action mappings, accompanied by a summary and assumptions. It proposes extractor/partition/window configuration against registered endpoint schemas; Rust validation checks the references. After review, **Apply reviewed setup** saves the pipeline versions directly; the administrator does not have to retype each field. It does not invent validated training records or automatically activate a model. Rust validates endpoint membership, field references, limits and shadow policy shape. The administrator reviews identity assumptions and business prerequisites before applying it. Pipeline creation and progress markers commit atomically. Applying an associated behavior policy prepares a snapshot and queues training using stable idempotency keys when data is available, then records training and isolated replay results. Empty or inadequate data is reported as needing attention. No module installation, server connection, model activation, or release deployment is exposed to the model.

## Validation

Run the existing Rust, Python, portal and native checks plus:

```bash
node --test ../pragma_change_sdks/node/test.mjs
python3 ../pragma_change_sdks/scripts/test-sdk-parity.py  # requires Node, Python, Rust and .NET 8
python3 scripts/test-remote-helper.py
# Requires .NET 8:
dotnet build ../pragma_change_sdks/csharp/PragmaChange.csproj
```

The helper test launches actual Nginx with isolated local files and verifies signing, minimal integration, observed-versus-on-disk identity, reload, rollback, and TCP rejection of management status. It does not by itself demonstrate SSH or VM isolation. Use the [remote-lab procedure](remote-lab.md) for a separate-machine acceptance run. Actual Claude calls require a configured account and are not made by automated tests.

### Implementation validation record (2026-10-10)

- Rust workspace tests and strict Clippy passed, including isolated PostgreSQL tests for collection privacy, conflicting duplicate delivery, correlation binding, durable jobs, chronological training, idempotent snapshots, shadow decisions, and replay that preserves live state while applying configured workflow rules.
- Python helper/SDK tests, Node SDK tests, and the existing Python trainer tests passed. An HTTP capture test confirmed identical v1 event JSON and allowlist filtering from Node.js, Python, C#, and Rust; the C# project compiled with .NET 8.
- The production portal build and the new desktop/mobile browser smoke test passed. The browser test uses mocked API responses to check navigation, operation requests and the reviewed-application gate; remote operations themselves were exercised through the actual API in the two-VM lab.
- The native four-worker Nginx test and helper lifecycle/failure tests passed. See [the two-VM record and measured performance limits](remote-lab.md) for real SSH deployment and offline enforcement evidence.

No production server was activated and the user's running management services were not restarted. Disposable guests and their test database schema were removed afterward. No live Claude request was made, SDK packages were not published, and container images were not built against a Docker daemon in this environment. Distribution-specific native compatibility, customer-labeled detection quality at a fixed false-alert budget, and sustained production load/fault testing remain deployment acceptance work. The sequence detector defaults to shadow mode for this reason.

### Offline recording and refactor validation (2026-10-10)

The four SDKs wrote both request and behavior files with matching normalized contents, allowlist filtering, private Unix permissions, counts and checksums; the shared Rust contract accepted all eight recordings. Streaming wire parity still passed. Request recordings replayed through the existing feature extractor with original body sizes preserved. Behavior database tests covered atomic rejection, concurrent retry idempotency, pseudonymization, untouched live history, training an imported snapshot, and the authenticated HTTP import route with more than 1 MiB of data. Tests use temporary schemas and listeners.

The 37 Rust workspace tests, strict Clippy, 21 Python tests, four Node tests, C# compilation, portal production build, and two mocked browser flows passed. The new recording checks are included in CI. The behavior refactor preserves public event/model contracts and moves unit/database tests into separate files. No customer runtime deployment or live provider call is part of these checks. The separate native/two-VM acceptance record above remains the evidence for the earlier remote-management milestone.

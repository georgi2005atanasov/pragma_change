# PragmaChange

Self-hosted behavioral security for HTTP APIs. Define endpoint features in a management portal, train on representative data, and evaluate requests locally inside an Nginx dynamic module. An optional application behavior service evaluates named actions and workflow sequences.

```text
Management: Portal → Rust API → PostgreSQL + artifacts
                            → Python forest trainer → release archive
API traffic: Client → Nginx + native Rust runtime → your application
Application events: Your server → optional behavior service + its own PostgreSQL
```

Nginx continues using its activated release when management services are offline. Its request path has no Python, database, or network dependency. Models are validated data files, not executable Python.

## Get the source

```bash
git clone https://github.com/georgi2005atanasov/pragma_change.git
cd pragma_change
```

Repository access is currently restricted. Use a GitHub account with access. The optional [reference clients and dataset recorders](https://github.com/georgi2005atanasov/pragma_change_sdks) live in a separate repository; they are examples to adapt, not required dependencies or published SDK products. The public documentation website is maintained separately from both repositories.

## 1. Start the management stack

Install Docker Engine with the Compose plugin, Python 3, and Git. On Linux, your account must have access to the Docker daemon. From this checkout:

```bash
python3 scripts/setup-env.py
docker compose up --build -d
docker compose ps
curl --fail http://127.0.0.1:8080/health
```

Open **http://localhost:3000**. Sign in with `ADMIN_USERNAME` and `ADMIN_PASSWORD` from the generated private `.env`. The setup script refuses to overwrite existing settings. The first build downloads dependencies and builds the portal, API, and trainer.

PostgreSQL and artifact files use named persistent volumes. Database and trainer ports are internal; the portal and API bind to loopback. Keep database and artifact backups together. For HTTPS, set `PUBLIC_ORIGIN` to the exact portal origin and `COOKIE_SECURE=true`. Changing environment credentials later does not reset an administrator already stored in the database.

```bash
docker compose logs --tail 100 api trainer portal
docker compose stop
# Start again without deleting data:
docker compose up -d
```

Avoid `docker compose down -v` unless you intend to delete stored data.

## 2. Configure one endpoint

In **Endpoints**, register an API host, then its method and path, for example `api.example.test`, `POST`, `/orders`. In **Feature pipelines**, select that endpoint and configure extractors, partition keys, windows, and ordered features. Save the immutable version.

A useful first pipeline selects `X-Client-ID` as a per-client partition and `/amount` from the JSON body, with request count and mean amount over a one-second window. [The sample pipeline](examples/pipeline.json) explains the structure; use the portal's real IDs and schema hash for imports.

New pipelines model one endpoint. Previously saved endpoint groups remain compatible, but new grouping is unavailable. [Partition and runtime semantics](docs/runtime.md) describe exact window, extraction, and warm-up behavior.

## 3. Import data and train

In **Datasets**, choose the saved pipeline and download its templates. Supply either:

- **Request-event JSONL:** timestamped requests. Rust derives partitions, windows, and features.
- **Feature-vector CSV:** finite numeric features with the exact saved column names and order, plus the matching dataset manifest.
- **An offline request recording:** `records.jsonl` plus its finalized SDK recording `manifest.json`.

You can create compatible files yourself or adapt the [reference recorders](https://github.com/georgi2005atanasov/pragma_change_sdks). No client library is required. [Recording and upload instructions](docs/sdk-recording.md) explain both paths and their limits.

Preview the import, correct every reported row error, and accept it. Training requires at least **20 complete vectors after warm-up**. Use representative normal traffic from the actual endpoint. Synthetic examples test the workflow, not detection quality.

In **Training & models**, select the dataset and start training. The Python trainer uses the first 80% of rows for training and the last 20% for evaluation. Rust validates the export and checks score parity before publishing the model. Use **Request replay** to inspect features and decisions with isolated state.

## 4. Export and activate a release

In **Releases**, select one model per endpoint and thresholds satisfying `0 <= check < block <= 1`. Download the archive. An anomaly score is not an attack probability; percentile suggestions are starting points and may need adjustment.

Follow the **[complete local Nginx walkthrough](docs/local-nginx.md)** to build the module, start a sample upstream, create the partition secret, install the archive, and verify a matching request. It uses loopback ports 18081 and 19000 without changing a system Nginx installation.

For an existing proxy, compile against its exact Nginx version, configure flags, architecture, and libc. The bundled **Nginx 1.28.0 Linux x86_64/glibc** build is a reference, not a universal module. `--with-compat` does not remove these requirements. The C adapter links the Rust static runtime into `ngx_http_pragmachange_module.so`; a generic Rust `.so` cannot be loaded directly by Nginx.

Model releases normally reload Nginx. Native module upgrades require a reviewed restart. Manual installation and rollback remain available through `scripts/deploy.py`; [remote server management](docs/expansion.md#remote-nginx-setup) adds pinned SSH, signed modules, configuration previews, durable jobs, serial rollouts, and verification of loaded releases.

## Decisions and application responsibilities

| Decision | Native Nginx behavior |
| --- | --- |
| `allow` | Forward with trusted risk metadata. |
| `check` | Forward; your application implements verification. |
| `block` | Stop with HTTP 403. |

Forwarded metadata uses `X-PragmaChange-Action`, `-Score`, `-Reason`, `-Pipeline`, and `-Model`. Incoming `X-PragmaChange-*` headers are stripped in protected locations. Unmatched routes pass without risk metadata. Accept these headers only from your trusted proxy.

Warm-up, missing inputs, body inspection failures, and exhausted state capacity produce `check` with no score. Threshold equality belongs to the higher action. The runtime observes requests before the upstream response; it does not measure response outcomes or business workflow state. Workers share history inside one Nginx instance, not across proxy hosts.

## Optional capabilities

- **[Application behavior](docs/expansion.md#application-behavior-service):** a separate Rust service and database collect authenticated application events, create immutable snapshots, train sequence models, and evaluate actions. New applications default to shadow mode.
- **[Offline recording](docs/sdk-recording.md):** collect local request or behavior files without running a service during collection. Request recordings import into Datasets; behavior recordings import into Behavior snapshots.
- **[Remote operations](docs/setup-commands.md#remote-nginx-commands-on-each-machine):** install the restricted host helper and configure pinned SSH and module signing before portal deployments.
- **[Assisted setup](docs/expansion.md#assisted-setup):** optionally configure Claude to propose validated pipelines and behavior policies. Review proposals before applying; deployment and model activation remain separate operations.

To start the optional behavior service with the management stack:

```bash
python3 scripts/setup-env.py --expansion
docker compose --env-file .env --env-file .env.expansion \
  -f compose.yaml -f compose.behavior.yaml up --build -d
```

Remote operations and Assisted setup additionally need a read-only credential-key mount in the API container. See [setup commands](docs/setup-commands.md); generating the key alone does not mount it.

## Native development and verification

The build images use Rust **1.98**, Node **24**, Python **3.12**, and PostgreSQL **18**. The portal pins pnpm **11.19.0**. The workspace declares Rust 1.88 as a minimum, but the documented build environment uses 1.98; older toolchains have not been revalidated against current locked dependencies. Use the checked-in lockfiles.

[Native setup commands](docs/setup-commands.md) cover database creation, environment variables, each service terminal, health checks, and optional services. Set `ARTIFACT_DIR` to an absolute persistent directory. Keep it with the matching database when backing up or restoring.

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 -m unittest discover -s scripts/tests
services/trainer/.venv/bin/pytest services/trainer/tests
pnpm --dir apps/portal build
```

Database tests require a disposable `TEST_DATABASE_URL`; full API/browser tests require their documented disposable stack. Native verification uses `scripts/build-nginx.sh` and `python3 scripts/test-nginx.py`. Tests for the reference clients live in their own repository. See [verification notes](docs/verification.md) and the [two-machine lab](docs/remote-lab.md) for test scope and limitations.

## Project map

| Directory | Responsibility |
| --- | --- |
| `apps/portal` | Management UI and server-side API proxy |
| `crates/api` | Configuration, datasets, jobs, releases, and remote operations |
| `crates/domain`, `crates/engine` | Pipeline contracts, extraction, windows, inference, and CLI |
| `crates/ffi`, `nginx` | C ABI and native Nginx module |
| `crates/behavior`, `crates/behavior-contract` | Application behavior service and portable contracts |
| `services/trainer` | Python Isolation Forest training |
| `deploy`, `scripts`, `docs`, `examples` | Containers, operations, contracts, and examples |

[Management OpenAPI](docs/openapi.json) · [Behavior OpenAPI](docs/behavior-openapi.json) · [Runtime contract](docs/runtime.md)

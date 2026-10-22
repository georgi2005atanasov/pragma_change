# Verification record

Verified locally on 2026-10-06 using Rust 1.98.1, Node 24.19, Python 3.12, PostgreSQL 18.6, and the reference Nginx 1.28.0 build.

| Check | Result |
|---|---|
| Rust formatting and Clippy with warnings denied | Passed |
| Rust engine tests | 11 passed: windows, all aggregates, ordering, partition encoding/isolation, missing/duplicate inputs, model validation, float32 split boundaries, eviction and replay determinism |
| PostgreSQL job recovery test | Passed: three retries, terminal failures, expired leases and fencing-attempt increments; isolated temporary schema |
| Python trainer tests | 3 passed: deterministic export, invalid matrices, service authentication |
| Deployment unit tests | 3 passed: checksum/link rejection, failed activation recovery, previous-release tracking |
| Management API integration | Passed against disposable `pragmachange_test`: login/CSRF, route conflicts, version creation, both imports, invalid rows/schemas, replay, training/parity, allow/check/block, release export and logout |
| Next.js production build | Passed |
| Browser workflow | Passed: create API and endpoint, configure pipeline, preview/import CSV, train model, create/download release, mobile viewport and logout |
| Native Nginx integration | Passed with four workers: shared counters, body preservation, header spoof stripping, all actions, malformed/oversized body fallback, model-only reload, configuration isolation, cardinality/event bounds and corrupt-release rejection |
| Real model deployment | Passed: API-trained and browser-exported models loaded in Nginx; actual archive validation, atomic install, reload and rollback |
| Docker Compose configuration | Validated with `docker compose config --quiet` |

Docker image builds and the complete Compose stack were not run in this session because the local user cannot access the Docker daemon. The individual services, native build, and integration flows were exercised directly; PostgreSQL ran in the user-provided Docker container. Existing databases were not modified.

The local portal is at `http://localhost:3000`, with the management API at `http://127.0.0.1:8080` and trainer at `http://127.0.0.1:8001`. The private, ignored `.env` holds administrator credentials and local configuration. The current Docker bridge address in `DATABASE_URL` may change if the PostgreSQL container is recreated.

Native outputs are `build/nginx/modules/ngx_http_pragmachange_module.so`, `build/nginx/sbin/nginx`, and `target/release/libpragma_ffi.so`. The module is built for this host; rebuild against the intended deployment's Nginx and libc environment. Browser screenshots are under `build/screenshots`. All build artifacts, traces, credentials, and test data are ignored by Git.

These checks establish functional behavior, not a production throughput target or detection accuracy. The shared state lock and serialized state transitions need workload-specific benchmarking. Unlabeled Isolation Forest holdout scores do not establish security efficacy.


## Usability and artifact recovery — 2026-10-08

The reported job `c6858066-0b6f-4d0a-998f-6095e5c1bccc` referenced the old synthetic `recovery` dataset, whose `vectors.json` file was missing. Its recorded SHA-256 matched the exact 20-row fixture in the worker test. Restoring those bytes and retrying through the management API succeeded on attempt one and published the model. All other recorded dataset/model files were present. The private local environment now uses an absolute artifact directory; the API logs that resolved path at startup.

Validation of the changes:

- Rust workspace: 18 tests passed, including six API database tests in isolated temporary schemas. Formatting and Clippy with warnings denied passed.
- New regression coverage: missing/corrupt dataset preflight, terminal worker failures and audit events, retry after restoration, contextual artifact writes, and model provenance outside the latest-200 list limits.
- Portal: TypeScript, production build, formatting, and two Playwright tests passed. Browser workflows use a separate API/portal and disposable database schema, not the normal workspace.
- Browser coverage: pipeline control heights/alignment, model details and scoring, tested thresholds passed into releases, invalid threshold feedback, audit search, sidebar persistence, mobile dismissal/focus, reduced motion, and narrow-screen overflow.
- API integration: authentication, immutable configuration, imports, replay, training/scoring parity, model provenance, release export, and logout passed.
- Deployment: API-trained and browser-exported bundles loaded in an isolated Nginx instance; installation, reload, and rollback passed. Three deployment unit tests passed. The user's proxy was not changed.

Screenshots are under `build/screenshots`, including `desktop-layout.png`, `collapsed-layout.png`, `mobile-navigation.png`, `mobile-layout.png`, `pipeline-controls.png`, and `model-details.png`. Production verification builds use `NEXT_DIST_DIR=.next-verification` to avoid replacing the running development portal's build output.

## Repository separation and documentation — 2026-10-10

The reference clients now live in the separate `pragma_change_sdks` repository. The main workspace, Docker build inputs, and CI no longer require that folder. The examples retain their public interfaces and bundle a checked snapshot of the portable Rust contract.

Validation for this change:

- Main and reference-client Rust workspaces passed formatting, strict Clippy, and workspace tests. Database-dependent tests returned early because `TEST_DATABASE_URL` was not provided; those database scenarios were not revalidated in this pass.
- Source-only copies of both repositories passed locked offline workspace checks in temporary directories without a sibling checkout. Main source export used Git's non-ignored file list.
- Main Python operations tests (16) and trainer tests (3) passed. Reference Python tests (5), Node tests (4), and Rust recording tests passed; the C# client built under .NET 8 without warnings or errors.
- All four clients passed streaming event parity and both request/behavior file parity, including privacy filtering, counts, checksums, Unix permissions, and shared import validation. The local system .NET launcher was broken; verification used an independently installed .NET 8 runtime.
- A fresh reference Nginx/module build succeeded. Native four-worker checks passed. Real saved releases passed install/reload/rollback after updating the deployment test to supply selected nested-body, query, and path fields from its pipeline rather than a hard-coded request.
- The documented local bootstrap commands were executed against disposable files, a matching saved release, and loopback listeners. Module loading, forwarding, and warm-up metadata passed; the example processes were stopped afterward.
- Environment generation, private file permissions, overwrite refusal, and base/behavior Compose configuration passed using disposable settings. Existing application credentials and data were retained.
- The separate website passed production build, TypeScript, and nine browser tests against the static output, including new guides, all five language/platform icons across four clients, downloads, section anchors, code copying, mobile layout, intermediate desktop widths, and visual fallbacks. Internal file links and anchors resolve.
- Both repositories' ignore rules retain source/configuration examples while excluding local credentials, recordings, build products, and test output. No commits, remotes, pushes, or package publication were performed.

Docker Compose configurations were validated, but Docker image builds and container startup remain unverified because this session cannot access the host Docker daemon. The website container launch returned a socket permission error. No host permissions were changed. The static website can be previewed independently; its container is configured for loopback port 3001.

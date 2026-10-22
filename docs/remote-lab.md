# Two-machine acceptance lab

This is a disposable KVM lab, not an installer for customer servers. It uses two separate Ubuntu Linux guest disks, OpenSSH transfer and loopback-only forwarded ports. No host deployment folder is mounted into either guest. The management API and durable deployment worker run in the management VM; Nginx, its restricted helper and a synthetic backend run in the proxy VM. The portal is verified separately with its production build and browser tests.

## Prerequisites

Use a Linux development host with `/dev/kvm`, QEMU, OpenSSH client, OpenSSL, Rust, Python 3, and an existing PostgreSQL connection reachable from the management guest. `DATABASE_URL` is used only to create an isolated `remote_lab_<uuid>` schema. The example never resets the existing schema. The guest must be able to route to that database address; a host URL containing `127.0.0.1` needs a guest-reachable address instead. The acceptance scripts assume the reference Nginx build and x86-64 guests.

Download the official Ubuntu 24.04 cloud image from [Ubuntu cloud images](https://cloud-images.ubuntu.com/noble/current/) to `build/remote-lab/base.img`. Check its hash against the publisher's `SHA256SUMS` and verify the publisher signature according to your image-trust policy. Only then create `build/remote-lab/verified`. The launcher intentionally does not silently download or trust a different image. Allow roughly 3 GiB RAM and several GiB free disk. The directory is private and contains disposable credentials and database connection metadata; do not publish it.

Build and run in this order from the repository root:

```bash
cargo build -p pragma-api -p pragma-engine
scripts/build-nginx.sh
python3 scripts/test-nginx.py
python3 scripts/remote/lab.py start
# Export DATABASE_URL from your local environment without printing it.
cargo run -p pragma-api --example lab_database
python3 scripts/test-remote-vms.py
cargo run -p pragma-api --example lab_database seed
python3 scripts/test-remote-vms-finish.py
```

The first script bootstraps only these disposable guests. It creates a dedicated deployment account without general sudo access, installs the privileged helper once, then starts a management API with its own admin credentials and external encryption key. The management worker pins the proxy's pre-provisioned host key. Native modules are built and signed off the proxy, then copied over SSH. Model releases are deterministic validated native-test fixtures inserted only into the isolated test schema, not through a production release-upload bypass.

The finish script verifies initial integration, a model-only graceful update, observed worker release identity, rollback, stale-configuration rejection, and refusal of an incorrect pinned host key. It powers off the entire management VM and checks that Nginx still blocks the fixture's protected request. A failed assertion exits nonzero. The finish script is a one-shot workflow; recreate guest overlays and schema before repeating it from scratch.

## Small topology benchmark

After the acceptance workflow leaves the management VM off and the proxy running:

```bash
cargo build -p pragma-behavior --example train_load
python3 scripts/benchmark-remote-vms.py
```

This restarts the management VM and compares inspection disabled, inspection enabled, and inspection while two management-side processes train the actual Rust sequence model on a synthetic 5,000-event trace. Each case uses four clients for approximately five seconds against a simple Python backend. It records throughput, latency percentiles, aggregate Nginx CPU time and summed process RSS. RSS includes shared pages more than once. It restores inspection afterward.

These are short functional measurements on a shared physical host, with QEMU user networking and a small synthetic model. They do not establish production capacity or isolation from shared-host CPU contention. Check errors as well as percentiles: failed requests are counted separately, not included in successful-response latency percentiles. Use repeated sustained trials, realistic models, a production backend, and dedicated hosts before making performance commitments.

## Cleanup

```bash
python3 scripts/remote/lab.py stop
cargo run -p pragma-api --example lab_database cleanup
```

After stopping both guests, remove their disposable `management.qcow2` and `proxy.qcow2` overlays before running setup again. They contain test credentials and configuration. Keep the verified base image if you want to repeat the test. The launcher does not modify any system Nginx installation on the development host, or stop the user's existing management services.

## Validation record — 2026-10-10

The two-VM workflow passed on separate KVM guests (2 vCPUs and 1,536 MiB each): registration, pinned SSH inspection, signed module transfer, reviewed integration, release activation/update/rollback, drift rejection, wrong-host-key rejection, and continued blocking with the management VM powered off.

A separate real-Nginx test additionally passed loaded-versus-on-disk release identity, mapped immutable native-module identity, public TCP status rejection, injected reload failure and recovery, and reconciliation after recovery itself was interrupted. Python tests cover conservative configuration parsing, included locations, TLS/routing preservation, operation idempotency, unresolved-job protection, and atomic-file failure under simulated disk exhaustion. These are not a substitute for a complete distribution/build compatibility matrix, real network/disk fault injection, or a production security review.

The short benchmark produced:

| Case | Requests/s | p95 / p99 (ms) | Nginx CPU seconds | Summed RSS MiB | Errors |
|---|---:|---:|---:|---:|---:|
| Inspection disabled | 2,758.1 | 2.31 / 3.33 | 1.74 | 12.43 | 1 |
| Inspection enabled | 2,507.6 | 1.75 / 2.17 | 1.84 | 14.43 | 0 |
| Inspection + management training | 1,180.0 | 2.40 / 2.86 | 1.36 | 14.43 | 4 |

Throughput was lower during the training case, consistent with shared physical-host contention despite separate guest processes. Other development checks were also running on this host, so this exploratory run cannot isolate training as the sole cause. CPU time is measured over each test's actual duration (5.00, 5.15, and 6.91 seconds respectively); it is not a normalized utilization comparison. The error counts prevent interpreting successful-response percentiles alone as reliability evidence. No production performance guarantee is inferred from this run.

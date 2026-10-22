# Apache HTTP Server integration

PragmaChange runs inside Apache as `mod_pragmachange.so`, using the same Rust detector and immutable model releases as Nginx. Install the native package once, then update models with a local deployment helper. No companion inference service is needed. Markov sequence detection remains a separate application behavior capability and does not alter proxy forest scores.

## Supported packages

| System | Architecture | Apache installation |
|---|---|---|
| Ubuntu 24.04 | amd64 / x86-64 | Distribution Apache 2.4 packages |
| Debian 12 | amd64 / x86-64 | Distribution Apache 2.4 packages |

Use the artifact built for your distribution. Each build records exact Apache/APR/libc package versions, source revision, and file checksums in `compatibility.json`. Package dependencies enforce the Apache module API and linked library requirements. Event, worker, and prefork MPMs are exercised; HTTP/2 is exercised with event and worker. Custom Apache builds, other distributions, ARM, Apache Traffic Server, Envoy, and HAProxy are not covered by these packages. This is not a universal `.so`.

CI produces `apache-debian12-amd64` and `apache-ubuntu2404-amd64` artifacts, each containing a `.deb`, checksum, module, compatibility record, and validation results. Download artifacts from a trusted build of the repository. A checksum detects corruption; it does not authenticate an untrusted distributor. No public package repository or automatically published release is configured.

## Install and configure

Verify the supplied package checksum, then install the matching file. Substitute its actual filename:

```sh
sha256sum -c libapache2-mod-pragmachange_VERSION_amd64.deb.sha256
sudo apt install ./libapache2-mod-pragmachange_VERSION_amd64.deb
sudo a2enmod pragmachange proxy proxy_http
sudo install -d -m 700 /etc/pragmachange/apache
# New installations only; never replace an existing partition secret:
sudo sh -c 'umask 077; set -C; openssl rand -hex 32 > /etc/pragmachange/apache/partition_secret'
```

Installing the package does not enable the module or restart Apache. Configure these global directives, outside virtual hosts, in an included configuration file:

```apache
PragmaChangeRelease /etc/pragmachange/apache/current/release.json
PragmaChangeSecretFile /etc/pragmachange/apache/partition_secret
PragmaChangeStateMiB 64
```

In the existing API virtual host, enable inspection only for the protected locations. Preserve existing authentication, TLS, and routing:

```apache
<Location /api/>
    PragmaChange On
</Location>
ProxyPass /api/ http://127.0.0.1:9000/api/
ProxyPassReverse /api/ http://127.0.0.1:9000/api/
```

For an existing backend pool, keep its `balancer://` rule. A minimal example also requires `proxy_balancer`, `lbmethod_byrequests`, and `slotmem_shm`:

```apache
<Proxy "balancer://application">
    BalancerMember http://127.0.0.1:9000
    BalancerMember http://127.0.0.1:9001
</Proxy>
ProxyPass /api/ balancer://application/api/
ProxyPassReverse /api/ balancer://application/api/
<Location /api/>
    PragmaChange On
</Location>
```

If `mod_cache` is loaded, configure `CacheQuickHandler Off` so cache handling does not bypass inspection. Request-body transformation filters before inspection are not supported; the module must inspect the body representation the backend receives. A filter that violates the bounded-read contract fails the request rather than forwarding truncated data. Configure trusted forwarding proxies through `mod_remoteip`; the detector uses Apache's resolved client address, never a raw caller-supplied `X-Forwarded-For` value.

For a release containing a named ingress policy, set `PragmaChangeGuard upload_guard` in global server configuration and enable `PragmaChange On` for every location it should protect. The guard checks decoded body size and multipart structure before a local handler such as `mod_php` or a reverse-proxy handler. It also runs when no endpoint model matches. A malformed or over-limit multipart request gets `403` with `X-PragmaChange-Decision: guard_block`; a scored model block gets `403` with `model_block` and its score. Allowed requests carry trusted `X-PragmaChange-Guard: allow` metadata. Apache refuses to load a guarded release without the directive, or with a missing or unsupported policy. Place `PragmaChange On` on all relevant paths; it does not protect locations where the module is disabled.

Add a dedicated loopback status listener from `deploy/apache.conf.example` (also installed under `/usr/share/doc/libapache2-mod-pragmachange/`). Choose an unused port and pass `--status-url` if it differs from the default:

```apache
Listen 127.0.0.1:19001
<VirtualHost 127.0.0.1:19001>
    ServerName pragmachange-status.local
    <Location /pragmachange-status>
        SetHandler pragmachange-status
        Require local
    </Location>
</VirtualHost>
```

The module checks actual local and peer socket addresses as well as Apache authorization. Do not expose this endpoint through a public reverse-proxy route. Status reports the serving worker's loaded release, module version, ABI, and PID, not the contents of a file on disk.

## Deploy, inspect, and roll back

In **Releases**, select **Apache HTTP Server** and download the model bundle. The API equivalent is `GET /api/v1/releases/{id}/download?target=apache`. Omitted targets still produce the unchanged Nginx archive format. The stored release and model identities are identical across targets.

```sh
sudo pragmachange-apache validate pragmachange_RELEASE_ID-apache.tar.gz
sudo pragmachange-apache install pragmachange_RELEASE_ID-apache.tar.gz
sudo pragmachange-apache status
sudo pragmachange-apache rollback
```

For a stopped Apache, append `--start` to the first installation. Other options are `--root`, `--apachectl`, `--apache-config`, `--validator`, `--status-url`, and `--timeout`. The configuration file must reference `current/release.json` beneath the selected root and the matching local partition secret.

The helper serializes local deployments, rejects unexpected archive entries and symlinks, verifies hashes and the Rust release contract, switches the release symlink atomically, checks Apache configuration, reloads, and observes the new release in a serving worker. A failure restores and verifies the previous release where one exists. If first activation succeeds but cannot be verified, there is no prior release to restore: the helper leaves the candidate link and reports that manual inspection is required. In-flight requests on draining workers may finish using the previous release.

Model-only graceful reloads retain compatible history across processes and threads. A new pipeline version gets separate history. Full restarts and secret rotation start warm-up again. Changing the shared-memory size or native module binary requires a full restart. History is never shared across separate proxy hosts. The portal's remote **Servers** workflow remains Nginx-only.

## Request behavior

- Matching protected requests use all existing extractors and window features, with unchanged score thresholds. An unmatched request receives any configured ingress guard but passes through without model risk metadata.
- `allow` and `check` forward the original request body and trusted `X-PragmaChange-*` metadata; `block` returns HTTP 403. Client-supplied risk headers are removed.
- Warm-up, capacity limits, invalid JSON, missing inputs, and oversized inspection bodies produce `check` without a score. The backend must implement verification. Invalid HTTP framing retains Apache's normal rejection behavior. Internal runtime failures return a server error.
- Body reading is bounded by the pipeline limit plus one byte. An over-limit request replays that prefix and streams the unread remainder to the backend.
- Apache merges repeated header values before normal request hooks. A selected header containing a comma, or a selected Cookie header containing a semicolon, conservatively produces an unscored `check`. This includes legitimate values using those separators. Prefer dedicated scalar identity headers; unselected headers are ignored. Query duplicates retain the Rust rejection behavior.
- Requests are inspected after Apache routing and access control, before either the local handler or proxy handler. Internal redirects cannot add a second history event; if an already-inspected request is remapped, its next inspection is an unscored check. Subrequests are not independently scored.

## Build and verify

```sh
# Build locally where Apache development headers and Rust are installed:
scripts/build-apache.sh
python3 scripts/test-apache.py --mpm event --http2 --benchmark
python3 scripts/test-apache.py --mpm worker --http2
python3 scripts/test-apache.py --mpm prefork

# Produce and test distribution-specific packages in isolated build stages:
docker buildx build --platform linux/amd64 -f deploy/Dockerfile.apache \
  --build-arg APACHE_BASE=debian:12-slim --target artifacts \
  --output type=local,dest=build/apache-debian12 .
docker buildx build --platform linux/amd64 -f deploy/Dockerfile.apache \
  --build-arg APACHE_BASE=ubuntu:24.04 --target artifacts \
  --output type=local,dest=build/apache-ubuntu2404 .
```

The acceptance test uses disposable ports and files, synthetic fixtures, and a local echo backend. It exercises body preservation, malformed/oversized/chunked requests, spoof stripping, all MPMs, HTTP/2 on threaded MPMs, shared windows and grouped compatibility, LRU pressure, reload/restart semantics, actual installation and rollback, and inaccessible public status. Benchmark output records latency percentiles, throughput, process CPU time, summed RSS, and errors with the module absent and enabled. These measurements do not establish security efficacy or production capacity.

## Verification record — 18 October 2026

Both packages were installed into fresh distribution runtime images without a compiler or Apache development headers. Installation left the module disabled. Native acceptance passed on the following builds:

| Build | Event | Worker | Prefork | HTTP/2 |
|---|---|---|---|---|
| Debian 12 / Apache 2.4.68-1~deb12u1 | Pass | Pass | Pass | Event and worker pass |
| Ubuntu 24.04 / Apache 2.4.58-1ubuntu8.17 | Pass | Pass | Pass | Event and worker pass |

The shared C interface was compared with deterministic engine replay for all 12 feature operations, including complete feature vectors, scores, decisions, partition keys, and warm-up. Both native adapters passed the same request fixtures for allow/check/block, body preservation, shared counts, grouped routes, capacity limits, and reload behavior. Their concurrent count fixture produced five allowed requests and fifteen blocked requests after warm-up. Apache additionally passed bounded body replay, chunked requests, HTTP/2, duplicate selected inputs, memory pressure, and local deployment recovery checks. These are fixture-based checks, not a claim of equivalence for every possible HTTP request; Apache's documented header ambiguity behavior still applies.

The browser workflow downloaded both archive types and verified identical model JSON. An actual trained Apache bundle from that workflow was installed, warmed up, scored, and rolled back on Ubuntu. Existing Nginx installation and remote deployment checks passed. The native acceptance instances had no management or training service available. Workspace tests, static checks, production website and portal builds, and browser tests also passed.

### Measured conditions

The measurements below used an isolated KVM guest with four virtual CPUs and 4 GiB RAM, distribution containers, the event MPM, three initial Apache children with four threads each, and a local Python echo backend. Each case started fresh, issued 400 HTTP/1.1 POST requests with concurrency eight, and used a 14-byte JSON body. The enabled case evaluated one current-value feature and a one-tree fixture model, with a 2 MiB shared-history limit. The baseline did not load `mod_pragmachange.so`; the harness checked the process mappings. The two distributions were measured sequentially.

| Case | Requests/s | p50 ms | p95 ms | p99 ms |
|---|---:|---:|---:|---:|
| Debian, module absent | 215.07 | 41.787 | 43.309 | 45.431 |
| Debian, enabled | 220.08 | 41.752 | 43.079 | 43.676 |
| Ubuntu, module absent | 221.89 | 41.382 | 42.590 | 43.041 |
| Ubuntu, enabled | 226.69 | 41.358 | 42.618 | 43.030 |

| Case | Apache CPU seconds | Summed RSS MiB | Errors |
|---|---:|---:|---:|
| Debian, module absent | 0.10 | 20.00 | 0 |
| Debian, enabled | 0.08 | 26.25 | 0 |
| Ubuntu, module absent | 0.07 | 21.89 | 0 |
| Ubuntu, enabled | 0.07 | 26.64 | 0 |

This short run is dominated by the local client/backend exchange and ordinary scheduling variation. It does not establish a speed improvement or a production overhead bound. CPU figures cover Apache's parent and children only; summed RSS counts shared pages more than once and is not unique physical memory. Repeat with representative models, window histories, payloads, backend behavior, and sustained load before capacity planning. Machine-readable results accompany the package artifacts under `validation/`; the CI matrix reruns these measurements for each build.

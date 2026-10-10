# Run the native module locally

This walkthrough uses the reference Nginx build on Linux, a sample upstream at `127.0.0.1:19000`, and a proxy at `127.0.0.1:18081`. It leaves system Nginx configuration alone. Keep both ports free. Run commands from the product checkout, using a path without spaces for the generated Nginx configuration.

First complete the README's management, dataset, training, and release steps. For the request below, register `api.example.test`, `POST /orders`, a client partition using `X-Client-ID`, and an amount field from `/amount`. Export the trained release to `artifacts/local-release.tar.gz`. If your pipeline differs, adapt the host, route, fields, and body to match it.

## Build

Install Rust 1.98, a C compiler, make, curl, OpenSSL, and libc development headers. On Debian/Ubuntu, the native build prerequisites are `build-essential curl ca-certificates openssl`; install Rust separately. Python 3 is needed for the installer and sample upstream.

```bash
scripts/build-nginx.sh
cargo build --locked --release -p pragma-engine
```

The build produces `build/nginx/sbin/nginx`, `build/nginx/modules/ngx_http_pragmachange_module.so`, and `target/release/pragma-cli`. The reference Nginx 1.28.0 build omits rewrite/gzip to minimize prerequisites. Rebuild for other target environments.

## Start the example upstream

In a separate terminal, from this checkout:

```bash
HOST=127.0.0.1 PORT=19000 python3 examples/upstream.py
```

Leave it running. It returns the received body and risk metadata so you can inspect forwarding. It is a demonstration backend, not a verification/challenge service.

## Prepare private state and configuration

Run once for a fresh local deployment. Existing secrets must be retained:

```bash
export PRAGMA_ROOT="$PWD"
export PRAGMA_LOCAL="$PRAGMA_ROOT/artifacts/local-nginx"
mkdir -p "$PRAGMA_LOCAL/logs"
chmod 700 "$PRAGMA_LOCAL"
(umask 077; set -C; openssl rand -hex 32 > "$PRAGMA_LOCAL/partition_secret")
cat > "$PRAGMA_LOCAL/nginx.conf" <<NGINX
load_module $PRAGMA_ROOT/build/nginx/modules/ngx_http_pragmachange_module.so;
worker_processes 2;
pid $PRAGMA_LOCAL/nginx.pid;
error_log $PRAGMA_LOCAL/logs/error.log;
events { worker_connections 256; }
http {
    access_log off;
    client_body_temp_path $PRAGMA_LOCAL/client_body_temp;
    proxy_temp_path $PRAGMA_LOCAL/proxy_temp;
    pragmachange_zone 64m;
    pragmachange_release $PRAGMA_LOCAL/current/release.json;
    pragmachange_secret_file $PRAGMA_LOCAL/partition_secret;
    server {
        listen 127.0.0.1:18081;
        server_name api.example.test;
        client_body_buffer_size 1m;
        client_max_body_size 1m;
        location / {
            pragmachange on;
            proxy_pass http://127.0.0.1:19000;
        }
    }
}
NGINX
```

The secret contains 64 hexadecimal characters. Do not include it in exported releases. Set body buffering at least as large as your pipeline inspection limit when JSON inspection matters; the 1 MiB values above are example policies. Disk-buffered, oversized-for-inspection, or malformed JSON bodies yield `check` and are still forwarded unless normal Nginx request limits reject them first.

## Validate and start

In the same terminal, using your downloaded archive:

```bash
python3 scripts/deploy.py validate artifacts/local-release.tar.gz \
  --root "$PRAGMA_LOCAL" --validator "$PRAGMA_ROOT/target/release/pragma-cli"
python3 scripts/deploy.py install artifacts/local-release.tar.gz \
  --root "$PRAGMA_LOCAL" --validator "$PRAGMA_ROOT/target/release/pragma-cli" \
  --nginx "$PRAGMA_ROOT/build/nginx/sbin/nginx" \
  --nginx-prefix "$PRAGMA_LOCAL" --nginx-config "$PRAGMA_LOCAL/nginx.conf" --start
```

The installer checks archive contents, checksums, model compatibility, and `nginx -t` before starting this isolated instance. It creates `current` atomically. Use `--start` only when this instance is stopped; omit it for subsequent installs so Nginx reloads.

## Send a matching request

```bash
curl --include http://127.0.0.1:18081/orders \
  -H 'Host: api.example.test' -H 'Content-Type: application/json' \
  -H 'X-Client-ID: local-example' --data '{"amount":42}'
```

A new client initially receives `check` during the longest configured window's warm-up. Repeat after that window. The sample backend returns `risk` metadata in its JSON response, including the action, reason, and score when available. These are upstream request headers, not automatically browser response headers. A scored request can allow, check, or receive HTTP 403 according to your data and thresholds. An unmatched route has no risk headers.

## Update, roll back, and stop

Install a different exported archive with the same command, omitting `--start`. Once a previous release exists:

```bash
python3 scripts/deploy.py rollback --root "$PRAGMA_LOCAL" \
  --nginx "$PRAGMA_ROOT/build/nginx/sbin/nginx" \
  --nginx-prefix "$PRAGMA_LOCAL" --nginx-config "$PRAGMA_LOCAL/nginx.conf"
# Validate the running instance's configuration:
"$PRAGMA_ROOT/build/nginx/sbin/nginx" -p "$PRAGMA_LOCAL" -c "$PRAGMA_LOCAL/nginx.conf" -t
# Stop only this example instance:
"$PRAGMA_ROOT/build/nginx/sbin/nginx" -p "$PRAGMA_LOCAL" -c "$PRAGMA_LOCAL/nginx.conf" -s quit
```

Stop the upstream with Ctrl+C. Retain the secret, releases, and logs if you intend to resume. Model-only updates for the same pipeline can retain state; changed pipeline versions warm up separately. Immediate activation failures restore the prior symlink. Native module upgrades require a restart, not just a model reload.

## Existing distribution Nginx

Inspect `nginx -V`, architecture, and libc on the target. Obtain the matching distribution source and build flags, retaining its required modules, then add `--add-dynamic-module=/absolute/path/to/pragma_change/nginx` with `PRAGMA_RUST_LIB=/absolute/path/to/pragma_change/target/release/libpragma_ffi.a`. Build that library with `cargo build --release -p pragma-ffi` first. Match distribution patches as well as version/configure flags. `--with-compat` is not universal ABI compatibility.

Install the resulting module, add `load_module` at the top level, set the three `pragmachange_*` HTTP directives, and enable `pragmachange on` in selected locations. Use actual deployment paths, permissions, upstreams, and body policies. Test before reloading. See [remote setup](setup-commands.md#remote-nginx-commands-on-each-machine) for the signed-module and restricted-SSH workflow.

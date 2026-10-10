"use client";
import { useState } from "react";
import type { Row } from "./ui";
function shellQuote(value: string) {
  return "'" + value.replaceAll("'", "'\\''") + "'";
}
function Command({ children }: { children: string }) {
  const [message, setMessage] = useState("");
  return (
    <div className="command">
      <pre>
        <code>{children}</code>
      </pre>
      <button
        className="secondary small"
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(children);
            setMessage("Copied");
          } catch {
            setMessage("Select and copy the command manually.");
          }
        }}
      >
        Copy
      </button>
      <small role="status">{message}</small>
    </div>
  );
}
export default function DeploymentGuide({ release }: { release: Row }) {
  const deployment = release.configuration.deployments[0];
  const pipeline = deployment?.pipeline;
  const archive = `pragmachange-${release.id}.tar.gz`;
  return (
    <details className="deployment-guide">
      <summary>How to deploy</summary>
      <p>
        The reusable <code>.so</code> runs inference. This bundle supplies the
        trained forest, feature configuration, and thresholds. Downloading it
        does not install it on Nginx.
      </p>
      <ol className="steps">
        <li>
          <b>Prepare the Nginx host once</b>
          <span>
            Build the module against your installed Nginx version and build
            flags. The reference build is Nginx 1.28.0 on Linux. Install
            pragma-cli on PATH, and copy scripts/deploy.py and the downloaded
            archive to this host. Commands below run from the repository root
            with permission to manage Nginx.
          </span>
        </li>
        <li>
          <b>Create the local secret once</b>
          <span>
            Keep an existing secret. Generate this only for a new installation;
            it is not included in the bundle.
          </span>
          <Command>{`sudo install -d -m 700 /etc/nginx/pragmachange\nsudo sh -c 'umask 077; set -C; openssl rand -hex 32 > /etc/nginx/pragmachange/partition_secret'`}</Command>
        </li>
        <li>
          <b>Configure your existing Nginx</b>
          <span>
            Replace the module path. Add the http directives inside your
            existing http block and enable the module in the location that
            proxies your API. Keep your existing proxy_pass.
          </span>
          <Command>{`# Top level\nload_module /absolute/path/ngx_http_pragmachange_module.so;\n\n# Inside http {}\npragmachange_zone 64m;\npragmachange_release /etc/nginx/pragmachange/current/release.json;\npragmachange_secret_file /etc/nginx/pragmachange/partition_secret;\nclient_body_buffer_size ${Math.max(...release.configuration.deployments.map((d: Row) => d.pipeline.limits.max_body_bytes))};\n\n# Inside the protected location {}\npragmachange on;`}</Command>
        </li>
        <li>
          <b>Validate, install, and reload</b>
          <span>
            Use the actual downloaded filename. Installation switches the
            current release, runs nginx -t, and reloads. For a stopped Nginx on
            first installation, append --start to the install command. For
            custom installations, supply --nginx, --nginx-config,
            --nginx-prefix, and --validator as needed.
          </span>
          <Command>{`python3 scripts/deploy.py validate ${archive} --root /etc/nginx/pragmachange\npython3 scripts/deploy.py install ${archive} --root /etc/nginx/pragmachange`}</Command>
        </li>
        <li>
          <b>Send a request through your proxy</b>
          <span>
            Replace the address, any path parameters, and request fields to
            match your endpoint and pipeline. Initial requests may return check
            while history warms up. Read X-PragmaChange-Action, Score, and
            Reason at your backend; they are upstream request headers. Block
            returns HTTP 403.
          </span>
          <Command>{`curl -i -X ${shellQuote(pipeline?.method || "POST")} ${shellQuote("http://127.0.0.1" + (pipeline?.path_template || "/"))} \\\n  -H ${shellQuote("Host: " + (pipeline?.api_host || "api.example.test"))} \\\n  -H 'Content-Type: application/json' \\\n  -H 'X-Client-ID: test-client'${["POST", "PUT", "PATCH"].includes(pipeline?.method) ? " \\\n  -d '{\"amount\":42}'" : ""}`}</Command>
        </li>
        <li>
          <b>Roll back if needed</b>
          <span>Available after a previous release has been installed.</span>
          <Command>
            {
              "python3 scripts/deploy.py rollback --root /etc/nginx/pragmachange"
            }
          </Command>
        </li>
      </ol>
      <p className="muted">
        The portal does not track which release is live on your proxy.
        Model-only updates preserve window history; changed pipeline versions
        warm up separately.
      </p>
    </details>
  );
}

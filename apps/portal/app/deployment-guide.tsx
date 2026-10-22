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
export type DeploymentTarget = "nginx" | "apache";

function ApacheGuide({ release }: { release: Row }) {
  const archive = `pragmachange-${release.id}-apache.tar.gz`;
  return (
    <details className="deployment-guide">
      <summary>How to deploy on Apache</summary>
      <p>
        The native module evaluates requests inside Apache HTTP Server. Your
        existing ProxyPass rules and backend pool stay in place. The model
        bundle is separate from the reusable module package.
      </p>
      <ol className="steps">
        <li>
          <b>Install the matching package once</b>
          <span>
            Use the verified libapache2-mod-pragmachange .deb for Ubuntu 24.04
            or Debian 12, Linux x86-64. Check its compatibility manifest and
            checksum. Package installation does not enable protection or restart
            Apache.
          </span>
          <Command>{`sudo apt install ./libapache2-mod-pragmachange_VERSION_amd64.deb\nsudo a2enmod pragmachange proxy proxy_http`}</Command>
        </li>
        <li>
          <b>Create the local secret once</b>
          <span>
            Keep your existing secret. Generate this only for a new
            installation.
          </span>
          <Command>{`sudo install -d -m 700 /etc/pragmachange/apache\nsudo sh -c 'umask 077; set -C; openssl rand -hex 32 > /etc/pragmachange/apache/partition_secret'`}</Command>
        </li>
        <li>
          <b>Configure your existing reverse proxy</b>
          <span>
            Adapt
            /usr/share/doc/libapache2-mod-pragmachange/apache.conf.example. It
            includes a private status listener and single-backend and
            load-balancer examples. Enable PragmaChange only in your API
            locations. Set CacheQuickHandler Off if mod_cache is loaded.
          </span>
          <Command>{`# Global Apache configuration\nPragmaChangeRelease /etc/pragmachange/apache/current/release.json\nPragmaChangeSecretFile /etc/pragmachange/apache/partition_secret\nPragmaChangeStateMiB 64\n\n# Inside the existing API VirtualHost\n<Location /api/>\n    PragmaChange On\n</Location>\n# Keep your existing ProxyPass /api/ ... rule.`}</Command>
        </li>
        <li>
          <b>Validate and activate this release</b>
          <span>
            Commands require permission to manage Apache. Use --start only for a
            stopped instance. Custom configurations can use --apachectl,
            --apache-config, and --status-url. Native module upgrades require a
            full restart; model updates use a graceful reload.
          </span>
          <Command>{`sudo pragmachange-apache validate ${archive}\nsudo pragmachange-apache install ${archive}\nsudo pragmachange-apache status`}</Command>
        </li>
        <li>
          <b>Verify a matching API request</b>
          <span>
            Read X-PragmaChange-Action, Score, and Reason at your backend. Allow
            and check forward the original body; block returns HTTP 403. Warm-up
            and invalid inspection inputs return check without a score. Your
            application must perform the required verification for check.
          </span>
        </li>
        <li>
          <b>Roll back to the previous model release</b>
          <Command>sudo pragmachange-apache rollback</Command>
        </li>
      </ol>
      <p className="muted">
        Apache setup is local and guided. The Servers section manages Nginx
        only. History is shared within one Apache instance; separate hosts warm
        up independently. Apache merges repeated headers, so ambiguous selected
        values return check without a score.
      </p>
    </details>
  );
}

export default function DeploymentGuide({
  release,
  target = "nginx",
}: {
  release: Row;
  target?: DeploymentTarget;
}) {
  if (target === "apache") return <ApacheGuide release={release} />;
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

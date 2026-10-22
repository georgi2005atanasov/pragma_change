"use client";
import { useEffect, useState } from "react";
import { Card, Empty, api, date, short, type Row } from "../ui";
import { useWorkspace } from "../workspace";

export default function ServersSection() {
  const { rows, run, busy } = useWorkspace();
  const [servers, setServers] = useState<Row[]>([]),
    [jobs, setJobs] = useState<Row[]>([]);
  const [allowRestart, setAllowRestart] = useState(false);
  const [selected, setSelected] = useState(""),
    [locations, setLocations] = useState<string[]>([]);
  const [upload, setUpload] = useState<Row | null>(null),
    [rollout, setRollout] = useState<string[]>([]);
  const [moduleId, setModuleId] = useState(""),
    [release, setRelease] = useState(""),
    [preview, setPreview] = useState("");
  async function refresh() {
    const [s, j] = await Promise.all([api("servers"), api("remote-jobs")]);
    setServers(s);
    setJobs(j);
  }
  useEffect(() => {
    run(refresh);
  }, []);
  useEffect(() => {
    if (!jobs.some((j) => ["queued", "running"].includes(j.status))) return;
    const timer = setInterval(() => run(refresh), 3000);
    return () => clearInterval(timer);
  }, [jobs]);
  const server = servers.find((s) => s.id === selected),
    discovery = server?.discovery;
  const serverJobs = jobs.filter((j) => j.server_id === selected);
  async function queue(operation: string, parameters: Row = {}) {
    await api(`servers/${selected}/jobs`, { operation, parameters });
    await refresh();
  }
  return (
    <>
      <Card title="Register an Nginx server">
        <p>
          The portal connects by SSH. Install the restricted host helper once,
          then register the dedicated account and independently verified host
          public key. Training stays on the management machine.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const form = e.currentTarget;
            const data = Object.fromEntries(new FormData(form));
            run(async () => {
              const result = await api("servers", {
                ...data,
                port: Number(data.port),
              });
              form.reset();
              setSelected(result.id);
              await refresh();
            }, "Server registered. Inspect it to test the connection.");
          }}
        >
          <div className="form-grid">
            <label>
              Name
              <input name="name" required maxLength={128} />
            </label>
            <label>
              Address
              <input name="address" required placeholder="proxy.internal" />
            </label>
            <label>
              SSH port
              <input
                name="port"
                type="number"
                defaultValue={22}
                min={1}
                max={65535}
                required
              />
            </label>
            <label>
              Deployment account
              <input name="username" required placeholder="pragma-deploy" />
            </label>
          </div>
          <label>
            Verified host public key
            <input
              name="host_key"
              required
              placeholder="ssh-ed25519 AAAA…"
              autoComplete="off"
            />
          </label>
          <label>
            Private SSH key
            <textarea
              name="private_key"
              required
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <p className="muted">
            Credentials are encrypted using the management server’s external
            credential key. They are never returned by the API.
          </p>
          <button disabled={busy}>Register server</button>
        </form>
      </Card>
      <Card title="Analyze an existing configuration">
        <label>
          Nginx configuration
          <input
            type="file"
            accept=".conf,.txt"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file)
                run(async () => {
                  if (file.size > 262144)
                    throw Error("Configuration exceeds 256 KiB");
                  setUpload(
                    await api("config-analysis", {
                      content: await file.text(),
                    }),
                  );
                });
            }}
          />
        </label>
        {upload && (
          <>
            <p>{upload.notice}</p>
            <pre>{JSON.stringify(upload.locations, null, 2)}</pre>
          </>
        )}
      </Card>
      <Card title="Inspect and prepare">
        <label>
          Server
          <select
            value={selected}
            onChange={(e) => {
              setSelected(e.target.value);
              setLocations([]);
              setModuleId("");
              setPreview("");
            }}
          >
            <option value="">Choose a server</option>
            {servers.map((s) => (
              <option value={s.id} key={s.id}>
                {s.name} · {s.address}
              </option>
            ))}
          </select>
        </label>
        {server && (
          <>
            <p>
              Desired release: {short(server.desired_release || "none")} ·
              Observed: {short(server.observed_release || "not verified")} ·
              Loaded module:{" "}
              {short(discovery?.observed?.module_id || "not verified")} · Last
              checked: {server.verified_at ? date(server.verified_at) : "never"}
            </p>
            {discovery?.configuration_drift && (
              <p className="error-text">
                The configuration changed since the previous verification.
                Review the current installation and prepare a fresh preview
                before deploying configuration changes.
              </p>
            )}
            <button disabled={busy} onClick={() => run(() => queue("inspect"))}>
              Test connection & inspect installation
            </button>
            {discovery && (
              <>
                <p>
                  {discovery.nginx} · {discovery.architecture} ·{" "}
                  {discovery.config}
                </p>
                <details>
                  <summary>Nginx build</summary>
                  <pre>{discovery.nginx_build}</pre>
                </details>
                <label>
                  Signed module package
                  <input
                    type="file"
                    accept=".gz,.tgz"
                    onChange={(e) => {
                      const file = e.target.files?.[0];
                      if (!file) return;
                      run(async () => {
                        if (file.size > 10 * 1024 * 1024)
                          throw Error(
                            "Package exceeds the 10 MiB portal upload limit",
                          );
                        const data = new Uint8Array(await file.arrayBuffer());
                        let raw = "";
                        for (
                          let offset = 0;
                          offset < data.length;
                          offset += 8192
                        )
                          raw += String.fromCharCode(
                            ...data.subarray(offset, offset + 8192),
                          );
                        await queue("stage_module", { package: btoa(raw) });
                      });
                    }}
                  />
                </label>
                <p className="muted">
                  Staging a package does not activate it. Inspect again after
                  staging to refresh available modules. Existing module upgrades
                  require a reviewed maintenance procedure.
                </p>
                <label>
                  Module
                  <select
                    value={moduleId}
                    onChange={(e) => setModuleId(e.target.value)}
                  >
                    <option value="">Choose a verified module</option>
                    {(discovery.modules || []).map((m: Row) => (
                      <option key={m.sha256} value={m.sha256}>
                        {short(m.sha256)} · ABI {m.abi}
                      </option>
                    ))}
                  </select>
                </label>
                <fieldset>
                  <legend>Locations to protect on first integration</legend>
                  {(discovery.locations || []).map((loc: Row) => (
                    <label className="checkbox" key={loc.id}>
                      <input
                        type="checkbox"
                        checked={locations.includes(loc.id)}
                        onChange={(e) =>
                          setLocations(
                            e.target.checked
                              ? [...locations, loc.id]
                              : locations.filter((x) => x !== loc.id),
                          )
                        }
                      />
                      {loc.file} · {loc.location}
                    </label>
                  ))}
                </fieldset>
                <button
                  disabled={busy || !moduleId || !locations.length}
                  onClick={() =>
                    run(() =>
                      queue("preview", {
                        fingerprint: discovery.fingerprint,
                        module_id: moduleId,
                        locations,
                      }),
                    )
                  }
                >
                  Prepare integration preview
                </button>
                <button
                  className="secondary"
                  disabled={busy || !moduleId || !discovery.observed}
                  onClick={() =>
                    run(() =>
                      queue("module_preview", {
                        fingerprint: discovery.fingerprint,
                        module_id: moduleId,
                      }),
                    )
                  }
                >
                  Prepare module upgrade (restart required)
                </button>
              </>
            )}
          </>
        )}
      </Card>
      {server && (
        <Card title="Review and deploy">
          <label>
            Integration preview
            <select
              value={preview}
              onChange={(e) => setPreview(e.target.value)}
            >
              <option value="">Use existing integration (model update)</option>
              {serverJobs
                .filter(
                  (j) =>
                    ["preview", "module_preview"].includes(j.operation) &&
                    j.status === "succeeded",
                )
                .map((j) => (
                  <option key={j.id} value={j.id}>
                    {date(j.created_at)} · {short(j.id)}
                  </option>
                ))}
            </select>
          </label>
          {preview && (
            <pre>{serverJobs.find((j) => j.id === preview)?.result?.diff}</pre>
          )}
          <label>
            Release
            <select
              value={release}
              onChange={(e) => setRelease(e.target.value)}
            >
              <option value="">Choose a release</option>
              {rows("releases").map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </select>
          </label>
          <label className="checkbox">
            <input
              type="checkbox"
              checked={allowRestart}
              onChange={(e) => setAllowRestart(e.target.checked)}
            />
            I acknowledge a restart and history reset if this reviewed module
            upgrade or rollback requires it.
          </label>
          <p>
            Activation validates the configuration, requests a graceful reload,
            and verifies the loaded release and configured health probes. A
            failed reload never triggers an automatic restart.
          </p>
          <button
            disabled={busy || !release || !discovery}
            onClick={() =>
              run(() =>
                queue("deploy", {
                  release_id: release,
                  fingerprint: discovery.fingerprint,
                  allow_restart: allowRestart,
                  ...(preview ? { preview_id: preview } : {}),
                }),
              )
            }
          >
            Deploy reviewed release
          </button>{" "}
          <button
            className="secondary"
            disabled={busy || !discovery?.rollback_available}
            onClick={() =>
              run(() =>
                queue("rollback", {
                  fingerprint: discovery.fingerprint,
                  allow_restart: allowRestart,
                }),
              )
            }
          >
            Roll back previous deployment
          </button>
        </Card>
      )}
      <Card title="Serial rollout">
        <p>
          Choose a release above, then select inspected servers. Deployment
          stops at the first failure or uncertain result.
        </p>
        {servers
          .filter((s) => s.discovery?.fingerprint)
          .map((s) => (
            <label className="checkbox" key={s.id}>
              <input
                type="checkbox"
                checked={rollout.includes(s.id)}
                onChange={(e) =>
                  setRollout(
                    e.target.checked
                      ? [...rollout, s.id]
                      : rollout.filter((id) => id !== s.id),
                  )
                }
              />
              {s.name}
            </label>
          ))}
        <button
          disabled={busy || !release || !rollout.length}
          onClick={() =>
            run(async () => {
              await api("remote-rollouts", {
                targets: rollout.map((id) => ({
                  server_id: id,
                  release_id: release,
                  fingerprint: servers.find((s) => s.id === id)!.discovery
                    .fingerprint,
                })),
              });
              await refresh();
            })
          }
        >
          Deploy release to selected servers in order
        </button>
      </Card>
      <Card title="Deployment history">
        {!serverJobs.length ? (
          <Empty>Select a server and inspect it to start.</Empty>
        ) : (
          serverJobs.map((job) => (
            <div className="release-bundle" key={job.id}>
              <b>
                {job.operation} · {job.status}
              </b>
              <small>
                {date(job.created_at)} · {short(job.id)}
              </small>
              {job.status === "unknown" && (
                <button
                  disabled={busy}
                  onClick={() =>
                    run(() => queue("reconcile", { target_id: job.id }))
                  }
                >
                  Reconcile remote operation
                </button>
              )}
              {job.result?.diff && (
                <details open>
                  <summary>Exact proposed configuration changes</summary>
                  <pre>{job.result.diff}</pre>
                </details>
              )}
              {job.result && (
                <details>
                  <summary>Results</summary>
                  <pre>{JSON.stringify(job.result, null, 2)}</pre>
                </details>
              )}
            </div>
          ))
        )}
      </Card>
    </>
  );
}

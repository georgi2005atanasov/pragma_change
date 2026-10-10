"use client";
import { useEffect, useState } from "react";
import { Card, Empty, api, type Row, date, short } from "../ui";
import { useWorkspace } from "../workspace";
export default function BehaviorSection() {
  const { run, busy } = useWorkspace();
  const [apps, setApps] = useState<Row[]>([]),
    [snapshots, setSnapshots] = useState<Row[]>([]),
    [models, setModels] = useState<Row[]>([]),
    [jobs, setJobs] = useState<Row[]>([]),
    [token, setToken] = useState(""),
    [replay, setReplay] = useState<Row | null>(null),
    [health, setHealth] = useState<Record<string, Row>>({});
  async function refresh() {
    const [a, s, m, j] = await Promise.all([
      api("behavior/apps"),
      api("behavior/snapshots"),
      api("behavior/models"),
      api("behavior/jobs"),
    ]);
    setApps(a);
    setSnapshots(s);
    setModels(m);
    setJobs(j);
  }
  useEffect(() => {
    run(refresh);
  }, []);
  useEffect(() => {
    if (!jobs.some((j) => ["queued", "running"].includes(j.status))) return;
    const timer = setInterval(() => run(refresh), 4000);
    return () => clearInterval(timer);
  }, [jobs]);
  return (
    <>
      <Card title="Connect an application">
        <p>
          Use a server-side SDK with identities from your authentication system.
          The separate behavior service collects events and checks action
          sequences. Nginx continues using its existing endpoint models.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const form = e.currentTarget;
            const name = new FormData(form).get("name");
            run(async () => {
              const r = await api("behavior/apps", { name });
              setToken(
                `Application: ${r.id}\nIngestion token: ${r.ingestion_token}`,
              );
              form.reset();
              await refresh();
            });
          }}
        >
          <label>
            Name
            <input name="name" required maxLength={128} />
          </label>
          <button disabled={busy}>Create application</button>
        </form>
        {token && (
          <div className="info">
            <p>Copy this application credential now. It is shown only once.</p>
            <pre>{token}</pre>
            <button className="secondary" onClick={() => setToken("")}>
              Hide credential
            </button>
          </div>
        )}
      </Card>
      <Card title="Collection and policies">
        {!apps.length ? (
          <Empty>No applications connected yet.</Empty>
        ) : (
          apps.map((a) => (
            <div className="release-bundle" key={a.id}>
              <b>{a.name}</b>
              <small>
                {a.id} · {a.collected_events} events ·{" "}
                {a.policy.enforce ? "Enforcing" : "Shadow mode"}
              </small>
              <p>
                Active model: {a.active_model ? short(a.active_model) : "none"}{" "}
                · Last received: {a.last_event ? date(a.last_event) : "never"}
              </p>
              <button
                className="secondary"
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    const result = await api(`behavior/apps/${a.id}/health`);
                    setHealth((old) => ({ ...old, [a.id]: result }));
                  })
                }
              >
                Check recent alert rate
              </button>
              {health[a.id] && (
                <p>
                  {health[a.id].unusual_transitions} unusual transitions in{" "}
                  {health[a.id].recent_evaluations} recent evaluations.{" "}
                  {health[a.id].review_baseline
                    ? "Review the baseline before enabling enforcement."
                    : "No sustained alert-rate review signal."}{" "}
                  {health[a.id].notice}
                </p>
              )}
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  const data = new FormData(e.currentTarget);
                  run(async () => {
                    const policy = JSON.parse(String(data.get("policy")));
                    await api(`behavior/apps/${a.id}/policy`, policy);
                    await refresh();
                  }, "Policy saved. The application must honor check and block decisions.");
                }}
              >
                <label>
                  Policy (allowlisted attributes, prerequisites, retention,
                  enforcement)
                  <textarea
                    name="policy"
                    defaultValue={JSON.stringify(a.policy, null, 2)}
                    rows={8}
                    spellCheck={false}
                  />
                </label>
                <button disabled={busy}>Save reviewed policy</button>
              </form>
              <button
                className="secondary"
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    await api(`behavior/apps/${a.id}/snapshots`, {});
                    await refresh();
                  })
                }
              >
                Create reproducible snapshot
              </button>
            </div>
          ))
        )}
      </Card>
      <Card title="Import an offline recording">
        <p>
          Upload the manifest and records from an SDK recorder. This creates a
          fixed training snapshot; it does not change live behavior history.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const form = e.currentTarget;
            const data = new FormData(form);
            run(async () => {
              const manifestFile = data.get("manifest") as File;
              const recordsFile = data.get("records") as File;
              if (
                manifestFile.size > 65536 ||
                recordsFile.size > 4 * 1024 * 1024
              )
                throw new Error(
                  "Recording exceeds the upload limit (64 KiB manifest, 4 MiB records).",
                );
              const manifest = JSON.parse(await manifestFile.text());
              await api(`behavior/apps/${data.get("application")}/imports`, {
                manifest,
                content: await recordsFile.text(),
              });
              form.reset();
              await refresh();
            }, "Recording imported. Select its snapshot below to train a sequence model.");
          }}
        >
          <label>
            Recording application
            <select name="application" required>
              <option value="">Select application</option>
              {apps.map((a) => (
                <option value={a.id} key={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Behavior manifest
            <input type="file" name="manifest" accept=".json" required />
          </label>
          <label>
            Behavior records
            <input
              type="file"
              name="records"
              accept=".jsonl,.ndjson"
              required
            />
          </label>
          <button disabled={busy || !apps.length}>Import recording</button>
        </form>
      </Card>
      <Card title="Snapshots and training">
        {snapshots.map((s) => (
          <div className="list-row" key={s.id}>
            <div>
              <b>{short(s.id)}</b>
              <small>
                {s.event_count} events · {date(s.created_at)}
              </small>
            </div>
            <button
              disabled={busy}
              onClick={() =>
                run(async () => {
                  await api(`behavior/snapshots/${s.id}/train`, {});
                  await refresh();
                })
              }
            >
              Train sequence model
            </button>
          </div>
        ))}
        {jobs.map((j) => (
          <p key={j.id}>
            {short(j.id)} · {j.status}
            {j.error ? ` · ${j.error}` : ""}
          </p>
        ))}
      </Card>
      <Card title="Model review">
        {models.map((m) => (
          <div className="release-bundle" key={m.id}>
            <b>{short(m.id)}</b>
            <p>
              {m.training_events} training events · {m.calibration_events}{" "}
              calibration events · {m.test_events} test events · Holdout flag
              rate {((Number(m.held_out_check_rate) || 0) * 100).toFixed(1)}% ·
              Check surprise {Number(m.check_surprise).toFixed(3)}
            </p>
            <p>
              Surprise is not an attack probability. Activation resets sequence
              history; new workflows must establish their boundaries. Keep the
              policy in shadow mode until representative evaluation is complete.
            </p>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                const snapshot_id = new FormData(e.currentTarget).get(
                  "snapshot_id",
                );
                run(async () =>
                  setReplay(
                    await api(`behavior/models/${m.id}/replay`, {
                      snapshot_id,
                    }),
                  ),
                );
              }}
            >
              <label>
                Replay snapshot
                <select name="snapshot_id" required>
                  {snapshots
                    .filter((s) => s.application_id === m.application_id)
                    .map((s) => (
                      <option key={s.id} value={s.id}>
                        {short(s.id)} · {s.event_count} events
                      </option>
                    ))}
                </select>
              </label>
              <button disabled={busy}>
                Replay without changing live history
              </button>
            </form>
            <button
              disabled={busy}
              onClick={() =>
                run(async () => {
                  await api(`behavior/models/${m.id}/activate`, {});
                  await refresh();
                })
              }
            >
              Activate reviewed model
            </button>
          </div>
        ))}
      </Card>
      {replay && (
        <Card title="Replay evidence">
          <p>
            {replay.would_check_or_block} of {replay.evaluated} events would
            require verification or blocking. {replay.notice}
          </p>
          <details>
            <summary>Decision evidence</summary>
            <pre>{JSON.stringify(replay.sample, null, 2)}</pre>
          </details>
        </Card>
      )}
    </>
  );
}

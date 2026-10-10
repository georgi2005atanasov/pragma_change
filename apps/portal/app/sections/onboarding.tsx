"use client";
import { useEffect, useState } from "react";
import { Card, api, type Row, date } from "../ui";
import { useWorkspace } from "../workspace";
export default function OnboardingSection() {
  const { run, busy } = useWorkspace();
  const [provider, setProvider] = useState<Row | null>(null),
    [jobs, setJobs] = useState<Row[]>([]),
    [approved, setApproved] = useState<Record<string, boolean>>({});
  async function refresh() {
    const [p, j] = await Promise.all([
      api("onboarding/provider"),
      api("onboarding/jobs"),
    ]);
    setProvider(p);
    setJobs(j);
  }
  useEffect(() => {
    run(refresh);
  }, []);
  useEffect(() => {
    if (
      !jobs.some(
        (j) =>
          ["queued", "running", "applying"].includes(j.status) ||
          (j.status === "applied" &&
            j.report?.completed?.training_job &&
            !j.report?.training_result),
      )
    )
      return;
    const timer = setInterval(() => run(refresh), 4000);
    return () => clearInterval(timer);
  }, [jobs]);
  return (
    <>
      <Card title="Claude connection">
        <p>
          Claude prepares configuration drafts. It has no SSH, deployment, or
          general command tools. Provider keys stay encrypted on the management
          server.
        </p>
        {provider && (
          <p>
            Configured: {provider.model} · at most {provider.daily_requests}{" "}
            requests per 24 hours
          </p>
        )}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const form = e.currentTarget,
              data = Object.fromEntries(new FormData(form));
            run(async () => {
              await api("onboarding/provider", {
                provider: "claude",
                ...data,
                daily_requests: Number(data.daily_requests),
              });
              form.reset();
              await refresh();
            });
          }}
        >
          <div className="form-grid">
            <label>
              Claude model identifier
              <input
                name="model"
                required
                placeholder="Your enabled Claude model"
              />
            </label>
            <label>
              Daily request limit
              <input
                name="daily_requests"
                type="number"
                min={1}
                max={100}
                defaultValue={10}
              />
            </label>
          </div>
          <label>
            API key
            <input name="api_key" type="password" autoComplete="off" required />
          </label>
          <button disabled={busy}>Save provider credential</button>
        </form>
      </Card>
      <Card title="Prepare a configuration">
        <p>
          Only the description below, registered endpoint schemas, and a
          configuration example are sent to Claude. Do not include credentials
          or raw customer requests. Each job makes at most one provider call,
          capped at 4,096 output tokens.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const form = e.currentTarget,
              data = new FormData(form);
            run(async () => {
              await api("onboarding/jobs", {
                description: data.get("description"),
                application_id: data.get("application_id") || null,
                share_description: data.get("share_description") === "on",
              });
              await refresh();
            });
          }}
        >
          <label>
            API and workflow description
            <textarea name="description" required maxLength={16000} rows={8} />
          </label>
          <label>
            Optional behavior application ID
            <input name="application_id" placeholder="Application UUID" />
          </label>
          <label className="checkbox">
            <input name="share_description" type="checkbox" required />I
            reviewed this description and approve sending it and the registered
            endpoint schemas to Claude.
          </label>
          <button disabled={busy || !provider}>Prepare validated draft</button>
        </form>
      </Card>
      <Card title="Drafts and operation reports">
        {jobs.map((j) => (
          <div className="release-bundle" key={j.id}>
            <b>
              {j.status} · {date(j.created_at)}
            </b>
            {j.error && <p className="error-text">{j.error}</p>}
            {j.draft && (
              <>
                <p>{j.draft.summary}</p>
                <details open>
                  <summary>Assumptions and mappings to review</summary>
                  <pre>
                    {JSON.stringify(
                      {
                        assumptions: j.draft.assumptions,
                        action_mappings: j.draft.action_mappings,
                      },
                      null,
                      2,
                    )}
                  </pre>
                </details>
                <details>
                  <summary>Validated configuration</summary>
                  <pre>{JSON.stringify(j.draft, null, 2)}</pre>
                </details>
              </>
            )}
            {j.status === "review" && (
              <>
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={!!approved[j.id]}
                    onChange={(e) =>
                      setApproved({ ...approved, [j.id]: e.target.checked })
                    }
                  />
                  I reviewed the identity mappings, prerequisites, and
                  assumptions.
                </label>
                <button
                  disabled={busy || !approved[j.id]}
                  onClick={() =>
                    run(async () => {
                      await api(`onboarding/jobs/${j.id}/apply`, {
                        confirm_assumptions: true,
                      });
                      await refresh();
                    })
                  }
                >
                  Apply reviewed setup
                </button>
              </>
            )}
            <details>
              <summary>Recorded results</summary>
              <pre>{JSON.stringify(j.report, null, 2)}</pre>
            </details>
          </div>
        ))}
      </Card>
    </>
  );
}

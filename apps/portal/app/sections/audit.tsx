"use client";
import { useState } from "react";
import { useWorkspace } from "../workspace";
import { Card, Empty, short, date, Badge } from "../ui";

export default function AuditSection() {
  const { rows, navigate } = useWorkspace();
  const [action, setAction] = useState("");
  const [query, setQuery] = useState("");
  const events = rows("audit-events");
  function resource(id: string) {
    for (const [table, section] of [
      ["apis", 1],
      ["endpoints", 1],
      ["pipelines", 2],
      ["datasets", 3],
      ["training-jobs", 4],
      ["models", 4],
      ["releases", 6],
    ] as const) {
      const r = rows(table).find((r) => r.id === id);
      if (r)
        return {
          section,
          name:
            r.name ||
            (r.configuration?.method
              ? `${r.configuration.method} ${r.configuration.path_template} · v${r.version}`
              : table === "training-jobs"
                ? `Training · ${rows("datasets").find((d) => d.id === r.dataset_id)?.name || short(id)}`
                : `Model ${short(id)}`),
        };
    }
    return null;
  }
  const filtered = events.filter(
    (a) =>
      (!action || a.action === action) &&
      `${a.action.replaceAll("_", " ")} ${a.actor} ${a.entity_id} ${resource(a.entity_id)?.name || ""} ${JSON.stringify(a.details)}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  return (
    <Card title="Administrative activity">
      <p className="muted">
        Latest 200 events, newest first. Filters apply to this loaded history.
        Times use your local timezone.
      </p>
      <div className="form-grid">
        <label>
          Search activity
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Dataset, resource ID, action, or details"
          />
        </label>
        <label>
          Action
          <select value={action} onChange={(e) => setAction(e.target.value)}>
            <option value="">All actions</option>
            {[...new Set(events.map((a) => a.action))].sort().map((a) => (
              <option key={a} value={a}>
                {a.replaceAll("_", " ")}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="muted">
        {filtered.length} of {events.length} loaded events
      </p>
      {filtered.length ? (
        <div className="table-wrap">
          <table className="audit-table">
            <thead>
              <tr>
                <th>Time</th>
                <th>Action</th>
                <th>Actor</th>
                <th>Resource</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((a) => {
                const target = resource(a.entity_id);
                return (
                  <tr key={a.id}>
                    <td>
                      <time dateTime={a.created_at}>{date(a.created_at)}</time>
                    </td>
                    <td>
                      <Badge
                        tone={
                          a.action === "training_failed"
                            ? "danger"
                            : a.action === "training_succeeded"
                              ? "success"
                              : "neutral"
                        }
                      >
                        {a.action === "training_failed"
                          ? "failed"
                          : a.action === "training_succeeded"
                            ? "succeeded"
                            : a.action.replaceAll("_", " ")}
                      </Badge>
                    </td>
                    <td>{a.actor}</td>
                    <td>
                      {target ? (
                        <button
                          className="resource-link"
                          onClick={() => navigate(target.section, a.entity_id)}
                        >
                          {target.name}
                        </button>
                      ) : (
                        "—"
                      )}
                      <small>
                        <code title={a.entity_id}>
                          {a.entity_id || "No resource"}
                        </code>
                      </small>
                    </td>
                    <td>
                      {Object.keys(a.details || {}).length ? (
                        <details>
                          <summary>View details</summary>
                          <pre>{JSON.stringify(a.details, null, 2)}</pre>
                        </details>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ) : (
        <Empty>
          {events.length
            ? "No activity matches these filters."
            : "No administrative activity recorded."}
        </Empty>
      )}
    </Card>
  );
}

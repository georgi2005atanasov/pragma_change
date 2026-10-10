"use client";
import { useState } from "react";
import { api, date, pipelineRoutes, pipelineLabel, type Row } from "./ui";
import { useWorkspace } from "./workspace";

export default function ModelDetails({ id }: { id: string }) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<Row | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const { rememberModel, navigate, choosePipeline, setDatasetId } =
    useWorkspace();
  async function toggle() {
    setOpen(!open);
    if (open || detail || loading) return;
    setLoading(true);
    setError("");
    try {
      const d = await api(`models/${id}`);
      rememberModel(d);
      setDetail(d);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }
  return (
    <div className="model-details">
      <button
        className="secondary small"
        aria-expanded={open}
        aria-controls={`model-details-${id}`}
        onClick={toggle}
      >
        {open ? "Less" : "More"}
      </button>
      {open && (
        <div id={`model-details-${id}`} className="detail-panel">
          {loading && <p role="status">Loading model details…</p>}
          {error && (
            <p role="alert" className="error-text">
              {error} Close and reopen to retry.
            </p>
          )}
          {detail && (
            <>
              <h4>Training provenance</h4>
              <dl>
                <dt>Dataset</dt>
                <dd>{detail.dataset.name}</dd>
                <dt>Format</dt>
                <dd>
                  {detail.dataset.format === "request_events"
                    ? "Request events (JSONL)"
                    : "Feature vectors (CSV)"}
                </dd>
                <dt>Imported</dt>
                <dd>{date(detail.dataset.created_at)}</dd>
                <dt>Complete vectors</dt>
                <dd>{detail.dataset.row_count}</dd>
                <dt>Training / holdout</dt>
                <dd>
                  {detail.evaluation.training_rows} /{" "}
                  {detail.evaluation.held_out_rows}
                </dd>
                <dt>Split</dt>
                <dd>First 80% / final 20%</dd>
                <dt>Endpoints</dt>
                <dd>
                  {pipelineRoutes(detail.pipeline.configuration).map(
                    (route) => (
                      <div key={route.endpoint_id}>
                        {route.method} {route.api_host}
                        {route.path_template}
                      </div>
                    ),
                  )}
                </dd>
                <dt>Pipeline</dt>
                <dd>
                  {pipelineLabel(detail.pipeline.configuration)} · Version{" "}
                  {detail.pipeline.version}
                </dd>
                <dt>Activity history</dt>
                <dd>
                  {!detail.pipeline.configuration.additional_endpoints?.length
                    ? "This endpoint only"
                    : detail.pipeline.configuration.history_scope ===
                        "per_endpoint"
                      ? "Separate per endpoint"
                      : "Combined across endpoints"}
                </dd>
                <dt>Trees</dt>
                <dd>{detail.training_job.parameters.n_estimators ?? 100}</dd>
                <dt>Requested samples/tree</dt>
                <dd>{detail.training_job.parameters.max_samples ?? 256}</dd>
                <dt>Effective samples/tree</dt>
                <dd>
                  {Math.min(
                    detail.training_job.parameters.max_samples ?? 256,
                    detail.evaluation.training_rows,
                  )}
                </dd>
                <dt>Random seed</dt>
                <dd>{detail.training_job.parameters.seed ?? 42}</dd>
                <dt>Model created</dt>
                <dd>{date(detail.created_at)}</dd>
              </dl>
              <b>Feature order</b>
              <ol className="feature-order">
                {detail.pipeline.configuration.features.map((f: Row) => (
                  <li key={f.name}>
                    <code>{f.name}</code>{" "}
                    <span className="muted">{f.operation}</span>
                  </li>
                ))}
              </ol>
              <div className="actions">
                <button
                  className="secondary small"
                  onClick={() => {
                    setDatasetId(detail.dataset.id);
                    navigate(3, detail.dataset.id);
                  }}
                >
                  View dataset →
                </button>
                <button
                  className="secondary small"
                  onClick={() => {
                    choosePipeline(detail.pipeline.id);
                    navigate(2, detail.pipeline.id);
                  }}
                >
                  View pipeline →
                </button>
              </div>
              <small>
                Dataset ID: <code>{detail.dataset.id}</code>
                <br />
                Model ID: <code>{detail.id}</code>
              </small>
            </>
          )}
        </div>
      )}
    </div>
  );
}

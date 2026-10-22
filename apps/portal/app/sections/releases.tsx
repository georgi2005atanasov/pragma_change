"use client";
import { useState } from "react";
import DeploymentGuide, { type DeploymentTarget } from "../deployment-guide";
import { useWorkspace } from "../workspace";
import {
  Card,
  Empty,
  api,
  short,
  date,
  modelLabel,
  thresholdError,
  modelsOverlap,
  pipelineRoutes,
} from "../ui";

export default function ReleasesSection() {
  const [target, setTarget] = useState<DeploymentTarget>("nginx");
  const {
    releaseName,
    setReleaseName,
    rows,
    selectedModels,
    testedThresholds,
    setSelectedModels,
    busy,
    run,
  } = useWorkspace();
  return (
    <>
      <Card title="Create a deployment release">
        <p>
          Select one model per endpoint and choose its thresholds. Previously
          saved grouped models remain available and cover all their listed
          routes. Selecting a conflicting model replaces the earlier selection.
        </p>
        <label>
          Release name
          <input
            value={releaseName}
            onChange={(e) => setReleaseName(e.target.value)}
            placeholder="Production baseline · v1"
          />
        </label>
        {rows("models").map((m) => (
          <div className="release-choice" key={m.id}>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={!!selectedModels[m.id]}
                onChange={(e) => {
                  const next = { ...selectedModels };
                  if (e.target.checked) {
                    for (const id of Object.keys(next))
                      if (
                        modelsOverlap(
                          rows("models").find((other) => other.id === id) || {},
                          m,
                        )
                      )
                        delete next[id];
                    next[m.id] = testedThresholds[m.id] || {
                      check: "",
                      block: "",
                    };
                  } else delete next[m.id];
                  setSelectedModels(next);
                }}
              />
              {modelLabel(m)}
              <small>
                {m.api_host} · {m.dataset_name}
              </small>
              {m.pipeline_configuration && (
                <small>
                  {pipelineRoutes(m.pipeline_configuration)
                    .map((r) => `${r.method} ${r.api_host}${r.path_template}`)
                    .join(" · ")}
                </small>
              )}
            </label>
            {selectedModels[m.id] && (
              <div>
                <small>
                  Model {short(m.id)}
                  {testedThresholds[m.id]
                    ? " · Filled from your last successful test; review before creating."
                    : " · Choose thresholds after reviewing the model."}
                </small>
                <div className="form-grid">
                  <label>
                    Check threshold
                    <input
                      type="number"
                      min={0}
                      max={1}
                      step="any"
                      value={selectedModels[m.id].check}
                      onChange={(e) =>
                        setSelectedModels({
                          ...selectedModels,
                          [m.id]: {
                            ...selectedModels[m.id],
                            check: e.target.value,
                          },
                        })
                      }
                    />
                  </label>
                  <label>
                    Block threshold
                    <input
                      type="number"
                      min={0}
                      max={1}
                      step="any"
                      value={selectedModels[m.id].block}
                      onChange={(e) =>
                        setSelectedModels({
                          ...selectedModels,
                          [m.id]: {
                            ...selectedModels[m.id],
                            block: e.target.value,
                          },
                        })
                      }
                    />
                  </label>
                </div>
                {thresholdError(
                  selectedModels[m.id].check,
                  selectedModels[m.id].block,
                ) && (
                  <p className="error-text" role="status">
                    {thresholdError(
                      selectedModels[m.id].check,
                      selectedModels[m.id].block,
                    )}
                  </p>
                )}
              </div>
            )}
          </div>
        ))}
        <div className="info">
          <b>Allow → Check → Block</b>
          <p>
            Require 0 ≤ check &lt; block ≤ 1. Check forwards to the normal
            backend with risk headers. Warm-up, missing fields, and exhausted
            state return check with a reason and no score.
          </p>
        </div>
        <button
          disabled={
            busy ||
            !releaseName ||
            !Object.keys(selectedModels).length ||
            Object.values(selectedModels).some(
              (v) => !!thresholdError(v.check, v.block),
            )
          }
          onClick={() =>
            run(async () => {
              await api("releases", {
                name: releaseName,
                models: Object.entries(selectedModels).map(
                  ([model_version_id, t]) => ({
                    model_version_id,
                    thresholds: {
                      check: Number(t.check),
                      block: Number(t.block),
                    },
                  }),
                ),
              });
              setReleaseName("");
              setSelectedModels({});
            }, "Release validated. Download the bundle to deploy it.")
          }
        >
          Validate & create release →
        </button>
      </Card>
      <Card title="Release bundles">
        <label>
          Deployment proxy
          <select
            value={target}
            onChange={(event) =>
              setTarget(event.target.value as DeploymentTarget)
            }
          >
            <option value="nginx">Nginx</option>
            <option value="apache">Apache HTTP Server</option>
          </select>
        </label>
        {rows("releases").length ? (
          rows("releases").map((r) => (
            <div className="release-bundle" id={`resource-${r.id}`} key={r.id}>
              <div className="list-row">
                <div>
                  <b>{r.name}</b>
                  <small>
                    {r.configuration.deployments.length} models ·{" "}
                    {r.configuration.deployments.reduce(
                      (sum: number, d: { pipeline: Record<string, any> }) =>
                        sum + pipelineRoutes(d.pipeline).length,
                      0,
                    )}{" "}
                    endpoints · {date(r.created_at)}
                  </small>
                </div>
                <a
                  className="button secondary small"
                  href={`/api/releases/${r.id}/download${target === "apache" ? "?target=apache" : ""}`}
                >
                  ↓ Download bundle
                </a>
              </div>
              <DeploymentGuide release={r} target={target} />
            </div>
          ))
        ) : (
          <Empty>
            No releases yet. Train a model and choose its thresholds first.
          </Empty>
        )}
      </Card>
      <Card title="From bundle to running proxy">
        <p>
          Install the compatible module for your selected proxy once. Each time
          you update your model, download a release and use its “How to deploy”
          guide to validate and activate it on your proxy.
        </p>
        <p className="muted">
          A release is a deployment package, not a live deployment. Inference
          runs locally in Nginx or Apache after installation, even when this
          portal is offline.
        </p>
      </Card>
    </>
  );
}

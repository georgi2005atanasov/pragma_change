"use client";
import { useEffect } from "react";
import ModelDetails from "../model-details";
import { useWorkspace } from "../workspace";
import {
  statusTone,
  Card,
  Select,
  Empty,
  Badge,
  api,
  short,
  date,
  modelLabel,
  thresholdError,
} from "../ui";

export default function TrainingSection() {
  const {
    prepareRelease,
    setTestedThresholds,
    datasetId,
    setDatasetId,
    rows,
    estimators,
    setEstimators,
    samples,
    setSamples,
    seed,
    setSeed,
    busy,
    run,
    error,
    counts,
    setModelId,
    setCheck,
    setBlock,
    setScoreResult,
    modelId,
    scoreVector,
    setScoreVector,
    check,
    block,
    scoreResult,
  } = useWorkspace();
  useEffect(() => {
    if (modelId)
      document.getElementById("model-test")?.scrollIntoView({
        block: "start",
        behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "instant"
          : "smooth",
      });
  }, [modelId]);
  const pipeline = rows("pipelines").find(
    (p) =>
      p.id ===
      rows("models").find((m) => m.id === modelId)?.pipeline_version_id,
  );
  return (
    <>
      <Card title="Train an Isolation Forest">
        <Select
          label="Validated dataset"
          value={datasetId}
          onChange={setDatasetId}
        >
          {rows("datasets").map((d) => (
            <option key={d.id} value={d.id}>
              {d.name} · {d.row_count} vectors
            </option>
          ))}
        </Select>
        <div className="form-grid three">
          <label>
            Trees
            <input
              type="number"
              min={1}
              max={512}
              value={estimators}
              onChange={(e) => setEstimators(Number(e.target.value))}
            />
          </label>
          <label>
            Samples per tree
            <input
              type="number"
              min={2}
              max={65536}
              value={samples}
              onChange={(e) => setSamples(Number(e.target.value))}
            />
          </label>
          <label>
            Random seed
            <input
              type="number"
              min={0}
              value={seed}
              onChange={(e) => setSeed(Number(e.target.value))}
            />
          </label>
        </div>
        <p className="muted">
          The first 80% of vectors train the model; the final 20% are held out.
          Jobs run in Python, then Rust validates the artifact and checks score
          parity before making it available.
        </p>
        <button
          disabled={busy || !datasetId}
          onClick={() =>
            run(async () => {
              await api("training-jobs", {
                dataset_id: datasetId,
                parameters: {
                  n_estimators: estimators,
                  max_samples: samples,
                  seed,
                },
              });
            }, "Training queued. This page refreshes while jobs are active.")
          }
        >
          Start training →
        </button>
      </Card>
      <Card title="Training jobs">
        {rows("training-jobs").length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Job</th>
                  <th>Dataset</th>
                  <th>Status</th>
                  <th>Attempts</th>
                  <th>Details</th>
                </tr>
              </thead>
              <tbody>
                {rows("training-jobs").map((j) => (
                  <tr key={j.id}>
                    <td>
                      <code>{short(j.id)}</code>
                    </td>
                    <td>
                      {
                        rows("datasets").find((d) => d.id === j.dataset_id)
                          ?.name
                      }
                    </td>
                    <td>
                      <Badge tone={statusTone(j.status)}>{j.status}</Badge>
                    </td>
                    <td>{j.attempts}</td>
                    <td>
                      {j.error && (
                        <small className="error-text">{j.error}</small>
                      )}
                      {j.status === "failed" && (
                        <button
                          className="secondary small"
                          onClick={() =>
                            run(async () => {
                              await api(`training-jobs/${j.id}/retry`, {});
                            })
                          }
                        >
                          Retry
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty>No training jobs yet.</Empty>
        )}
      </Card>
      <Card title="Model comparison">
        <p>
          Higher scores indicate unusual behavior. These distributions are from
          unlabeled held-out data and do not measure attack detection accuracy.
        </p>
        <div className="model-grid">
          {rows("models").map((m) => (
            <div className="model-card" id={`resource-${m.id}`} key={m.id}>
              <div className="card-title">
                <b>{modelLabel(m)}</b>
                <Badge tone="success">validated</Badge>
              </div>
              <small>
                {m.dataset_name} · {date(m.created_at)}
              </small>
              <div
                className="histogram"
                aria-label="Held-out anomaly score histogram"
              >
                {m.evaluation.histogram.counts.map((n: number, i: number) => (
                  <div
                    key={i}
                    title={`${m.evaluation.histogram.edges[i].toFixed(2)}: ${n} vectors`}
                    style={{
                      height: `${Math.max(2, (100 * n) / Math.max(...m.evaluation.histogram.counts, 1))}%`,
                    }}
                  />
                ))}
              </div>
              <div className="histogram-axis">
                <span>0 · typical</span>
                <span>1 · unusual</span>
              </div>
              <p>
                {m.evaluation.training_rows} training /{" "}
                {m.evaluation.held_out_rows} held-out rows
              </p>
              <dl>
                <dt>Suggested check</dt>
                <dd>{m.evaluation.suggested_check.toFixed(4)}</dd>
                <dt>Suggested block</dt>
                <dd>{m.evaluation.suggested_block.toFixed(4)}</dd>
              </dl>
              <button
                className="secondary small"
                onClick={() => {
                  setModelId(m.id);
                  setCheck(String(m.evaluation.suggested_check));
                  setBlock(String(m.evaluation.suggested_block));
                  setScoreResult(null);
                }}
              >
                Test this model
              </button>
              <button
                className="secondary small"
                onClick={() => prepareRelease(m)}
              >
                Create release →
              </button>
              <ModelDetails id={m.id} />
            </div>
          ))}
        </div>
        {!rows("models").length && (
          <Empty>Validated models appear after training completes.</Empty>
        )}
      </Card>
      {modelId && (
        <div id="model-test">
          <Card title={`Test model ${short(modelId)}`}>
            <p>
              Supply a numeric vector in the pipeline’s exact feature order.
              Scores are tested without production window state.
            </p>
            {pipeline && (
              <div className="vector-preview">
                Feature order
                <code>
                  [{" "}
                  {pipeline.configuration.features
                    .map((f: { name: string }) => f.name)
                    .join(", ")}{" "}
                  ]
                </code>
              </div>
            )}
            <label>
              Vector (JSON array)
              <input
                className="mono"
                value={scoreVector}
                onChange={(e) => setScoreVector(e.target.value)}
                placeholder="[12, 150]"
              />
            </label>
            <div className="form-grid">
              <label>
                Check threshold
                <input
                  type="number"
                  step="any"
                  min={0}
                  max={1}
                  value={check}
                  onChange={(e) => setCheck(e.target.value)}
                />
              </label>
              <label>
                Block threshold
                <input
                  type="number"
                  step="any"
                  min={0}
                  max={1}
                  value={block}
                  onChange={(e) => setBlock(e.target.value)}
                />
              </label>
            </div>
            {thresholdError(check, block) && (
              <p className="muted">{thresholdError(check, block)}</p>
            )}
            <button
              disabled={busy || !scoreVector || !!thresholdError(check, block)}
              onClick={() =>
                run(async () => {
                  const result = await api(`models/${modelId}/score`, {
                    vector: JSON.parse(scoreVector),
                    thresholds: { check: Number(check), block: Number(block) },
                  });
                  setScoreResult(result);
                  setTestedThresholds((old) => ({
                    ...old,
                    [modelId]: { check, block },
                  }));
                })
              }
            >
              Score vector
            </button>
            {scoreResult && (
              <div className="result">
                <Badge tone={statusTone(scoreResult.action)}>
                  {scoreResult.action}
                </Badge>
                <strong>{scoreResult.risk_score?.toFixed(6)}</strong>
                <span>{scoreResult.reason}</span>
              </div>
            )}
          </Card>
        </div>
      )}
    </>
  );
}

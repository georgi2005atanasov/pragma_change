"use client";
import { useWorkspace } from "../workspace";
import {
  statusTone,
  Card,
  Select,
  Badge,
  api,
  modelLabel,
  thresholdError,
} from "../ui";
import type { Row, Feature } from "../ui";
export default function ReplaySection() {
  const {
    pipelineSelector,
    modelId,
    setModelId,
    rows,
    pipelineId,
    selectedPipeline,
    check,
    setCheck,
    block,
    setBlock,
    busy,
    run,
    setReplayText,
    setNotice,
    replayText,
    setReplayResult,
    replayResult,
  } = useWorkspace();
  return (
    <Card title="Replay requests">
      <p>
        Inspect partition hashes, window counts, the ordered vector, and the
        model decision. Each replay starts with fresh state and cannot change
        your Nginx history.
      </p>
      <div className="two-col compact">
        {pipelineSelector}
        <Select label="Optional model" value={modelId} onChange={setModelId}>
          {rows("models")
            .filter((m) => m.pipeline_version_id === pipelineId)
            .map((m) => (
              <option key={m.id} value={m.id}>
                {modelLabel(m)} · {m.dataset_name}
              </option>
            ))}
        </Select>
      </div>
      {selectedPipeline && (
        <div className="vector-preview">
          <code>
            [
            {selectedPipeline.configuration.features
              .map((f: Feature) => f.name)
              .join(", ")}
            ]
          </code>
        </div>
      )}
      {modelId && (
        <div className="form-grid">
          <label>
            Check threshold
            <input
              type="number"
              step="any"
              value={check}
              onChange={(e) => setCheck(e.target.value)}
            />
          </label>
          <label>
            Block threshold
            <input
              type="number"
              step="any"
              value={block}
              onChange={(e) => setBlock(e.target.value)}
            />
          </label>
        </div>
      )}
      <button
        className="secondary small"
        disabled={!pipelineId || busy}
        onClick={() =>
          run(async () => {
            const t = await api(`pipelines/${pipelineId}/template`);
            setReplayText(
              (t.request_events || [t.request_event])
                .map((event: unknown) => JSON.stringify(event))
                .join("\n"),
            );
            setNotice(t.notes.join(" "));
          })
        }
      >
        Load request template
      </button>
      <label>
        Timestamped request events (one JSON object per line)
        <textarea
          rows={10}
          value={replayText}
          onChange={(e) => setReplayText(e.target.value)}
          spellCheck={false}
        />
      </label>
      {modelId && thresholdError(check, block) && (
        <p className="muted">{thresholdError(check, block)}</p>
      )}
      <button
        disabled={
          busy ||
          !pipelineId ||
          !replayText ||
          (!!modelId && !!thresholdError(check, block))
        }
        onClick={() =>
          run(async () =>
            setReplayResult(
              await api(`pipelines/${pipelineId}/replay`, {
                events: replayText
                  .split("\n")
                  .filter((s) => s.trim())
                  .map((s) => JSON.parse(s)),
                model_version_id: modelId || null,
                thresholds: modelId
                  ? { check: Number(check), block: Number(block) }
                  : null,
              }),
            ),
          )
        }
      >
        Replay & inspect →
      </button>
      {replayResult &&
        replayResult.results.map((r: Row, i: number) => (
          <details className="replay-event" key={i} open={i === 0}>
            <summary>
              Request {i + 1} · {r.timestamp_ms} ms{" "}
              <Badge
                tone={statusTone(r.decision?.action || r.features?.reason)}
              >
                {r.decision?.action || r.features?.reason}
              </Badge>
            </summary>
            <pre>{JSON.stringify(r, null, 2)}</pre>
          </details>
        ))}
    </Card>
  );
}

"use client";
import { useState } from "react";
import { useWorkspace } from "../workspace";
import { Card, Empty, api, download, date } from "../ui";

export default function DatasetsSection() {
  const [recordingManifest, setRecordingManifest] = useState<Record<
    string,
    any
  > | null>(null);
  const {
    pipelineSelector,
    datasetName,
    setDatasetName,
    format,
    setFormat,
    setPreview,
    pipelineId,
    setPipelineId,
    busy,
    run,
    manifest,
    setNotice,
    setError,
    setContent,
    setManifest,
    content,
    rows,
    preview,
    setDatasetId,
    setSection,
  } = useWorkspace();
  return (
    <>
      <Card title="Import a training dataset">
        <div className="two-col compact">
          {pipelineSelector}
          <label>
            Dataset name
            <input
              value={datasetName}
              onChange={(e) => setDatasetName(e.target.value)}
              placeholder="Normal traffic · September"
            />
          </label>
        </div>
        <div className="choice-grid">
          {[
            [
              "request_events",
              "Request events",
              "Timestamped JSONL requests. Rust extracts fields, groups partitions, updates windows, and creates every feature vector.",
            ],
            [
              "feature_vectors",
              "Feature vectors",
              "Numeric CSV rows that are already aggregated. Every column must match the pipeline’s feature names and order.",
            ],
          ].map(([key, title, description]) => (
            <button
              key={key}
              className={`choice ${format === key ? "selected" : ""}`}
              onClick={() => {
                setFormat(key);
                setPreview(null);
              }}
            >
              <b>{title}</b>
              <span>{description}</span>
            </button>
          ))}
        </div>
        <div className="info">
          <b>
            {format === "request_events"
              ? "How request replay works"
              : "Why the schema must match"}
          </b>
          <p>
            {format === "request_events"
              ? "Use one JSON request per line with timestamp_ms, endpoint_id, method, path, headers, query, client_ip, body, and body_size. Train on representative traffic for the selected pipeline’s endpoint. Extra fields inside body, headers, and query are allowed. Requests are sorted chronologically; include enough history for warm-up. Previously saved grouped versions retain their original endpoint membership and history rules. No production traffic is captured automatically."
              : "A model learns positions in a numeric vector. Reordering columns or changing a partition/window changes their meaning. Supply the schema hash from the same pipeline version. Do not mix versions or substitute zero for missing values."}
          </p>
        </div>
        <div className="actions">
          <button
            className="secondary small"
            disabled={!pipelineId || busy}
            onClick={() =>
              run(async () => {
                const t = await api(`pipelines/${pipelineId}/template`);
                download(
                  "dataset-manifest.json",
                  JSON.stringify(t.manifest, null, 2),
                  "application/json",
                );
                download(
                  format === "request_events"
                    ? "request-events.jsonl"
                    : "feature-vectors.csv",
                  format === "request_events"
                    ? (t.request_events || [t.request_event])
                        .map((event: unknown) => JSON.stringify(event))
                        .join("\n") + "\n"
                    : t.csv,
                );
                setNotice(
                  t.notes.join(" ") ||
                    "Templates downloaded. Replace example values with representative data.",
                );
              })
            }
          >
            ↓ Download templates
          </button>
          <label className="file-label">
            Choose file
            <input
              type="file"
              accept=".csv,.jsonl,.ndjson,.txt"
              onChange={async (e) => {
                const file = e.target.files?.[0];
                if (file) {
                  if (file.size > 15 * 1024 * 1024) {
                    setError("File exceeds 15 MiB");
                    return;
                  }
                  setContent(await file.text());
                  setPreview(null);
                  if (!datasetName) setDatasetName(file.name);
                }
              }}
            />
          </label>
        </div>
        <label className="file-label">
          SDK recording manifest (optional)
          <input
            type="file"
            accept=".json"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (!file) return;
              run(async () => {
                if (file.size > 65536)
                  throw new Error("Manifest exceeds 64 KiB");
                const value = JSON.parse(await file.text());
                if (
                  value.format_version !== 1 ||
                  !value.finalized ||
                  value.config?.format !== "request_events" ||
                  !value.pipeline_version_id ||
                  !value.schema_hash
                )
                  throw new Error(
                    "Choose a finalized request recording manifest. Import behavior recordings in Behavior.",
                  );
                setRecordingManifest(value);
                setPipelineId(value.pipeline_version_id);
                setManifest(value.schema_hash);
                setFormat("request_events");
                setPreview(null);
              });
            }}
          />
        </label>
        {recordingManifest && (
          <p>
            Recording: {recordingManifest.row_count} requests. Select its
            records.jsonl file above. The server verifies the checksum and
            pipeline binding before import.
            <button
              className="secondary small"
              onClick={() => {
                setRecordingManifest(null);
                setPreview(null);
              }}
            >
              Clear recording manifest
            </button>
          </p>
        )}
        <label>
          Schema hash
          <input
            className="mono"
            value={manifest}
            onChange={(e) => {
              setManifest(e.target.value);
              setPreview(null);
            }}
            placeholder="Exact hash from the pipeline’s manifest"
          />
        </label>
        <label>
          {format === "request_events"
            ? "Request events (JSONL)"
            : "Feature vectors (CSV)"}
          <textarea
            value={content}
            onChange={(e) => {
              setContent(e.target.value);
              setPreview(null);
            }}
            rows={9}
            spellCheck={false}
            placeholder={
              format === "request_events"
                ? '{"timestamp_ms":1000,"endpoint_id":"…","method":"POST","path":"/orders","headers":{"X-Client-ID":"a"},"body_size":120}'
                : "requests_per_minute,body_bytes\n12,120\n14,135"
            }
          />
        </label>
        <div className="actions">
          <button
            disabled={busy || !pipelineId || !content || !datasetName}
            onClick={() =>
              run(async () =>
                setPreview(
                  await api("datasets/preview", {
                    pipeline_version_id: pipelineId,
                    name: datasetName,
                    format,
                    content,
                    schema_hash: manifest,
                    recording_manifest: recordingManifest,
                  }),
                ),
              )
            }
          >
            Preview & validate
          </button>
          {preview?.valid && (
            <button
              className="secondary"
              disabled={busy}
              onClick={() =>
                run(async () => {
                  await api("datasets", {
                    pipeline_version_id: pipelineId,
                    name: datasetName,
                    format,
                    content,
                    schema_hash: manifest,
                    recording_manifest: recordingManifest,
                  });
                  setPreview(null);
                }, "Dataset imported and ready for training.")
              }
            >
              Accept import →
            </button>
          )}
        </div>
        {preview && (
          <div className="preview">
            <div className="stats mini">
              <div>
                <b>{preview.source_rows}</b>
                <span>Source rows</span>
              </div>
              <div>
                <b>{preview.warmup_rows}</b>
                <span>Warm-up excluded</span>
              </div>
              <div>
                <b>{preview.training_rows}</b>
                <span>Training vectors</span>
              </div>
            </div>
            {!preview.valid && (
              <div className="error">
                Import requires no row errors and at least 20 complete feature
                vectors.
              </div>
            )}
            {preview.errors.length > 0 && (
              <pre>{JSON.stringify(preview.errors, null, 2)}</pre>
            )}
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    {preview.feature_names.map((n: string) => (
                      <th key={n}>{n}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {preview.preview.map((v: number[], i: number) => (
                    <tr key={i}>
                      {v.map((x, j) => (
                        <td key={j}>{x.toFixed(3)}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </Card>
      <Card title="Imported datasets">
        {rows("datasets").length ? (
          rows("datasets").map((d) => (
            <div className="list-row" id={`resource-${d.id}`} key={d.id}>
              <div>
                <b>{d.name}</b>
                <small>
                  {d.row_count} vectors · {d.format} · {date(d.created_at)}
                </small>
              </div>
              <button
                className="secondary small"
                onClick={() => {
                  setDatasetId(d.id);
                  setSection(4);
                }}
              >
                Train model →
              </button>
            </div>
          ))
        ) : (
          <Empty>Your imported datasets will appear here.</Empty>
        )}
      </Card>
    </>
  );
}

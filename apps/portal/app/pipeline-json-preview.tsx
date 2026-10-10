"use client";
import { useState } from "react";
import type { Config, Row } from "./ui";

type PreviewMode = "request" | "pipeline";

function pointerParts(pointer: string): string[] | null {
  if (!pointer.startsWith("/") || /~(?![01])/.test(pointer)) return null;
  return pointer
    .slice(1)
    .split("/")
    .map((part) => part.replaceAll("~1", "/").replaceAll("~0", "~"));
}

function sampleRequest(config: Config, endpoint: Row) {
  const notes: string[] = [];
  const headers: Record<string, string> = Object.create(null);
  const query: Record<string, string> = Object.create(null);
  const body: Record<string, unknown> = Object.create(null);
  const numericSources = new Set(
    config.features
      .filter((feature) =>
        ["sum", "mean", "min", "max", "stddev", "current_value"].includes(
          feature.operation,
        ),
      )
      .map((feature) => feature.source),
  );

  for (const extractor of config.extractors) {
    const { kind, name, pointer } = extractor.source;
    if (kind === "client_ip") continue;
    if (kind === "path_parameter") {
      if (name && !endpoint.path_template.includes(`{${name}}`))
        notes.push(`Path parameter “${name}” is not in the endpoint path.`);
      continue;
    }
    if (kind === "header" || kind === "query_parameter") {
      if (!name) {
        notes.push(`Enter a name for the ${kind.replace("_", " ")} field.`);
        continue;
      }
      (kind === "header" ? headers : query)[name] = numericSources.has(
        extractor.name,
      )
        ? "42"
        : "example";
      continue;
    }
    if (kind === "json_pointer") {
      const parts = pointerParts(pointer || "");
      if (!parts || parts.some((part) => !part)) {
        notes.push(`Enter a valid JSON Pointer for “${extractor.name}”.`);
        continue;
      }
      let parent: Record<string, unknown> = body;
      let conflict = false;
      for (const part of parts.slice(0, -1)) {
        if (!Object.hasOwn(parent, part)) parent[part] = Object.create(null);
        if (
          typeof parent[part] !== "object" ||
          parent[part] === null ||
          Array.isArray(parent[part])
        ) {
          conflict = true;
          break;
        }
        parent = parent[part] as Record<string, unknown>;
      }
      const leaf = parts.at(-1)!;
      if (
        conflict ||
        (Object.hasOwn(parent, leaf) && typeof parent[leaf] === "object")
      ) {
        notes.push(`“${pointer}” overlaps another sample field.`);
        continue;
      }
      parent[leaf] = numericSources.has(extractor.name) ? 42 : "example";
    }
  }

  const serializedBody = JSON.stringify(body);
  return {
    event: {
      timestamp_ms: 1000,
      endpoint_id: endpoint.id,
      method: endpoint.method,
      path: endpoint.path_template.replace(/\{[^{}]+\}/g, "example"),
      headers,
      query,
      client_ip: "192.0.2.1",
      body,
      body_size: new TextEncoder().encode(serializedBody).length,
    },
    notes,
  };
}

export default function PipelineJsonPreview({
  config,
  endpoint,
  apiHost,
}: {
  config: Config;
  endpoint?: Row;
  apiHost?: string;
}) {
  const [mode, setMode] = useState<PreviewMode>("request");
  const [exampleEndpointId, setExampleEndpointId] = useState("");
  const endpoints: Row[] = endpoint
    ? [
        endpoint,
        ...(config.additional_endpoints || []).map((route) => ({
          id: route.endpoint_id,
          ...route,
        })),
      ]
    : [];
  const exampleEndpoint =
    endpoints.find((e) => e.id === exampleEndpointId) || endpoint;
  const request = exampleEndpoint
    ? sampleRequest(config, exampleEndpoint)
    : null;
  const { id: _generatedId, ...draft } = config;
  const pipeline = endpoint
    ? {
        ...draft,
        api_host: apiHost || "",
        method: endpoint.method,
        path_template: endpoint.path_template,
      }
    : draft;
  const value = mode === "request" ? request?.event : pipeline;
  return (
    <aside className="pipeline-json-panel" aria-label="Live JSON preview">
      <div className="pipeline-json-heading">
        <div>
          <span className="eyebrow">LIVE PREVIEW</span>
          <h3>See the JSON shape</h3>
        </div>
        <span className="preview-live-dot" aria-hidden="true" />
      </div>
      <div className="preview-tabs" role="tablist" aria-label="JSON view">
        <button
          type="button"
          role="tab"
          id="request-preview-tab"
          aria-selected={mode === "request"}
          aria-controls="pipeline-preview-content"
          className={mode === "request" ? "selected" : ""}
          onClick={() => setMode("request")}
        >
          Request example
        </button>
        <button
          type="button"
          role="tab"
          id="pipeline-preview-tab"
          aria-selected={mode === "pipeline"}
          aria-controls="pipeline-preview-content"
          className={mode === "pipeline" ? "selected" : ""}
          onClick={() => setMode("pipeline")}
        >
          Pipeline JSON
        </button>
      </div>
      {mode === "request" && endpoints.length > 1 && (
        <label>
          Example endpoint
          <select
            value={exampleEndpoint?.id || ""}
            onChange={(e) => setExampleEndpointId(e.target.value)}
          >
            {endpoints.map((e) => (
              <option key={e.id} value={e.id}>
                {e.method} {e.path_template}
              </option>
            ))}
          </select>
        </label>
      )}
      <div
        id="pipeline-preview-content"
        role="tabpanel"
        aria-labelledby={
          mode === "request" ? "request-preview-tab" : "pipeline-preview-tab"
        }
      >
        {value ? (
          <pre
            aria-label={mode === "request" ? "Request JSON" : "Pipeline JSON"}
          >
            <code>{JSON.stringify(value, null, 2)}</code>
          </pre>
        ) : (
          <p className="preview-placeholder">
            Choose an endpoint to see the request that your dataset should
            contain.
          </p>
        )}
      </div>
      <p className="preview-explanation">
        {mode === "request"
          ? "Example of one JSONL dataset row. Replace sample values and timestamp with real requests."
          : "Fields of the version to save. The server assigns its ID and schema hash."}
      </p>
      {mode === "request" &&
        request?.notes.map((note, index) => (
          <p className="preview-note" key={`${note}-${index}`}>
            {note}
          </p>
        ))}
    </aside>
  );
}

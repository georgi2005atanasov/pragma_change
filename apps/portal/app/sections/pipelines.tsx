"use client";
import { useWorkspace } from "../workspace";
import PipelineJsonPreview from "../pipeline-json-preview";
import {
  Card,
  Select,
  Empty,
  Badge,
  api,
  short,
  ops,
  needsSource,
  current,
  pipelineRoutes,
  pipelineLabel,
} from "../ui";

const limitHelp: Record<string, [string, string]> = {
  max_body_bytes: [
    "Body inspection size (bytes)",
    "Maximum request body inspected. Larger or disk-buffered bodies produce check. Keep Nginx client_body_buffer_size at least this large.",
  ],
  max_partitions: [
    "Tracked groups",
    "Maximum retained groups (for example clients) per pipeline version. Evicted groups warm up again.",
  ],
  max_events_per_partition: [
    "Events per group",
    "Maximum retained request events per group. Truncating history restarts warm-up.",
  ],
  max_partition_bytes: [
    "Storage per group (bytes)",
    "Maximum serialized history per group. Exceeding this produces check with a capacity reason. Nginx’s shared zone separately bounds total memory.",
  ],
};
export default function PipelinesSection() {
  const {
    navigate,
    choosePipeline,
    config,
    changeConfig,
    rows,
    busy,
    run,
    setPipelineId,
    setManifest,
    setConfig,
    setNotice,
  } = useWorkspace();
  const endpoint = rows("endpoints").find((e) => e.id === config.endpoint_id);
  const apiHost = rows("apis").find((a) => a.id === endpoint?.api_id)?.host;
  const legacyGroupDraft = !!config.additional_endpoints?.length;
  const availableEndpoints = [...rows("endpoints")];
  // Saved groups may contain endpoints older than the latest endpoint listing.
  for (const route of pipelineRoutes(config)) {
    if (
      route.endpoint_id &&
      !availableEndpoints.some((e) => e.id === route.endpoint_id)
    )
      availableEndpoints.push({ id: route.endpoint_id, ...route });
  }
  function selectEndpoint(id: string) {
    const e = availableEndpoints.find((e) => e.id === id);
    changeConfig({
      endpoint_id: id,
      api_host:
        rows("apis").find((a) => a.id === e?.api_id)?.host || e?.api_host || "",
      method: e?.method || "POST",
      path_template: e?.path_template || "/",
      additional_endpoints: [],
      history_scope: "shared",
    });
  }
  return (
    <>
      <Card
        title="Pipeline builder"
        aside={<Badge>NEW IMMUTABLE VERSION</Badge>}
      >
        <p>
          A partition <b>definition</b> describes how to group traffic, such as
          a client header plus a JSON account field. Each actual combination of
          values creates a separate partition <b>instance</b>.
        </p>
        <div className="pipeline-builder-layout">
          <div className="pipeline-builder-fields">
            <label>
              Pipeline name (optional)
              <input
                value={config.name || ""}
                maxLength={128}
                placeholder="Order activity"
                onChange={(e) => changeConfig({ name: e.target.value })}
              />
            </label>
            <Select
              label="Endpoint"
              value={legacyGroupDraft ? "" : config.endpoint_id}
              onChange={selectEndpoint}
            >
              {availableEndpoints.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.method} {e.path_template} ·{" "}
                  {rows("apis").find((a) => a.id === e.api_id)?.host ||
                    e.api_host}
                </option>
              ))}
            </Select>
            <p className="muted">
              Each pipeline learns the normal behavior of one endpoint.
              Partitions separate accounts or clients within that endpoint.
            </p>
            {legacyGroupDraft && (
              <p className="info">
                This draft comes from a previously saved endpoint group. Choose
                one endpoint before saving a new version. The saved group and
                its models remain unchanged.
              </p>
            )}
            <h3>1. Extract request fields</h3>
            <p className="muted">
              Add only fields used by features or partition keys. Other request
              fields can stay in your dataset. Requests to this endpoint must
              supply these fields. Names use snake_case; JSON fields use JSON
              Pointer notation, for example /account/id.
            </p>
            {config.extractors.map((e, i) => (
              <div className="builder-row" key={i}>
                <label>
                  Name
                  <input
                    aria-label={`Extractor ${i + 1} name`}
                    value={e.name}
                    onChange={(ev) =>
                      changeConfig({
                        extractors: config.extractors.map((x, j) =>
                          j === i ? { ...x, name: ev.target.value } : x,
                        ),
                      })
                    }
                  />
                </label>
                <label>
                  Source
                  <select
                    value={e.source.kind}
                    onChange={(ev) =>
                      changeConfig({
                        extractors: config.extractors.map((x, j) =>
                          j === i
                            ? {
                                ...x,
                                source:
                                  ev.target.value === "client_ip"
                                    ? { kind: ev.target.value }
                                    : ev.target.value === "json_pointer"
                                      ? {
                                          kind: ev.target.value,
                                          pointer: "/id",
                                        }
                                      : {
                                          kind: ev.target.value,
                                          name: "",
                                        },
                              }
                            : x,
                        ),
                      })
                    }
                  >
                    {[
                      "header",
                      "json_pointer",
                      "query_parameter",
                      "path_parameter",
                      "client_ip",
                    ].map((k) => (
                      <option key={k}>{k}</option>
                    ))}
                  </select>
                </label>
                <label>
                  {e.source.kind === "json_pointer" ? "JSON Pointer" : "Field"}
                  <input
                    disabled={e.source.kind === "client_ip"}
                    value={e.source.pointer ?? e.source.name ?? ""}
                    onChange={(ev) =>
                      changeConfig({
                        extractors: config.extractors.map((x, j) =>
                          j === i
                            ? {
                                ...x,
                                source: {
                                  ...x.source,
                                  [x.source.kind === "json_pointer"
                                    ? "pointer"
                                    : "name"]: ev.target.value,
                                },
                              }
                            : x,
                        ),
                      })
                    }
                  />
                </label>
                <button
                  className="remove"
                  aria-label="Remove extractor"
                  onClick={() =>
                    changeConfig({
                      extractors: config.extractors.filter((_, j) => j !== i),
                    })
                  }
                >
                  ×
                </button>
              </div>
            ))}
            <button
              className="secondary small"
              onClick={() =>
                changeConfig({
                  extractors: [
                    ...config.extractors,
                    {
                      name: `field_${config.extractors.length + 1}`,
                      source: { kind: "header", name: "" },
                    },
                  ],
                })
              }
            >
              + Add field
            </button>
            <h3>2. Compose partitions</h3>
            {config.partitions.map((p, i) => (
              <div className="builder-row" key={i}>
                <label>
                  Definition name
                  <input
                    value={p.name}
                    onChange={(e) =>
                      changeConfig({
                        partitions: config.partitions.map((x, j) =>
                          j === i ? { ...x, name: e.target.value } : x,
                        ),
                      })
                    }
                  />
                </label>
                <label>
                  Ordered components
                  <input
                    value={p.components.join(",")}
                    placeholder="client,account"
                    onChange={(e) =>
                      changeConfig({
                        partitions: config.partitions.map((x, j) =>
                          j === i
                            ? {
                                ...x,
                                components: e.target.value
                                  .split(",")
                                  .map((c) => c.trim()),
                              }
                            : x,
                        ),
                      })
                    }
                  />
                  <small>
                    Comma-separated extractor names. Ordering is part of the
                    definition.
                  </small>
                </label>
                <button
                  className="remove"
                  aria-label="Remove partition"
                  onClick={() =>
                    changeConfig({
                      partitions: config.partitions.filter((_, j) => j !== i),
                    })
                  }
                >
                  ×
                </button>
              </div>
            ))}
            <button
              className="secondary small"
              onClick={() =>
                changeConfig({
                  partitions: [
                    ...config.partitions,
                    {
                      name: `partition_${config.partitions.length + 1}`,
                      components: [],
                    },
                  ],
                })
              }
            >
              + Add partition
            </button>
            <h3>3. Define trailing windows</h3>
            <p className="muted">
              A window includes the current request and history newer than its
              duration. A new partition warms up for a full window before
              scoring.
            </p>
            <p className="muted">
              Interarrival features measure gaps between request arrivals.
              Execution duration and the order of different operations are not
              measured.
            </p>
            {config.windows.map((w, i) => (
              <div className="builder-row" key={i}>
                <label>
                  Name
                  <input
                    value={w.name}
                    onChange={(e) =>
                      changeConfig({
                        windows: config.windows.map((x, j) =>
                          j === i ? { ...x, name: e.target.value } : x,
                        ),
                      })
                    }
                  />
                </label>
                <label>
                  Duration (milliseconds)
                  <input
                    type="number"
                    min={1}
                    max={86400000}
                    value={w.duration_ms}
                    onChange={(e) =>
                      changeConfig({
                        windows: config.windows.map((x, j) =>
                          j === i
                            ? {
                                ...x,
                                duration_ms: Number(e.target.value),
                              }
                            : x,
                        ),
                      })
                    }
                  />
                </label>
                <button
                  className="remove"
                  aria-label="Remove window"
                  onClick={() =>
                    changeConfig({
                      windows: config.windows.filter((_, j) => j !== i),
                    })
                  }
                >
                  ×
                </button>
              </div>
            ))}
            <button
              className="secondary small"
              onClick={() =>
                changeConfig({
                  windows: [
                    ...config.windows,
                    {
                      name: `window_${config.windows.length + 1}`,
                      duration_ms: 10000,
                    },
                  ],
                })
              }
            >
              + Add window
            </button>
            <h3>4. Build the ordered feature vector</h3>
            {config.features.map((f, i) => (
              <div className="feature-row" key={i}>
                <span className="index">{i + 1}</span>
                <label>
                  Name
                  <input
                    value={f.name}
                    onChange={(e) =>
                      changeConfig({
                        features: config.features.map((x, j) =>
                          j === i ? { ...x, name: e.target.value } : x,
                        ),
                      })
                    }
                  />
                </label>
                <label>
                  Operation
                  <select
                    value={f.operation}
                    onChange={(e) =>
                      changeConfig({
                        features: config.features.map((x, j) =>
                          j === i
                            ? {
                                ...x,
                                operation: e.target.value,
                                source: needsSource(e.target.value)
                                  ? config.extractors[0]?.name || ""
                                  : null,
                                partition: current(e.target.value)
                                  ? null
                                  : config.partitions[0]?.name || "",
                                window: current(e.target.value)
                                  ? null
                                  : config.windows[0]?.name || "",
                              }
                            : x,
                        ),
                      })
                    }
                  >
                    {ops.map((op) => (
                      <option key={op}>{op}</option>
                    ))}
                  </select>
                </label>
                {needsSource(f.operation) && (
                  <Select
                    label="Source"
                    value={f.source || ""}
                    onChange={(v) =>
                      changeConfig({
                        features: config.features.map((x, j) =>
                          j === i ? { ...x, source: v } : x,
                        ),
                      })
                    }
                  >
                    {config.extractors.map((e) => (
                      <option key={e.name}>{e.name}</option>
                    ))}
                  </Select>
                )}
                {!current(f.operation) && (
                  <>
                    <Select
                      label="Partition"
                      value={f.partition || ""}
                      onChange={(v) =>
                        changeConfig({
                          features: config.features.map((x, j) =>
                            j === i ? { ...x, partition: v } : x,
                          ),
                        })
                      }
                    >
                      {config.partitions.map((e) => (
                        <option key={e.name}>{e.name}</option>
                      ))}
                    </Select>
                    <Select
                      label="Window"
                      value={f.window || ""}
                      onChange={(v) =>
                        changeConfig({
                          features: config.features.map((x, j) =>
                            j === i ? { ...x, window: v } : x,
                          ),
                        })
                      }
                    >
                      {config.windows.map((e) => (
                        <option key={e.name}>{e.name}</option>
                      ))}
                    </Select>
                  </>
                )}
                <button
                  className="remove"
                  aria-label="Move feature up"
                  disabled={i === 0}
                  onClick={() => {
                    const fs = [...config.features];
                    [fs[i - 1], fs[i]] = [fs[i], fs[i - 1]];
                    changeConfig({ features: fs });
                  }}
                >
                  ↑
                </button>
                <button
                  className="remove"
                  aria-label="Remove feature"
                  onClick={() =>
                    changeConfig({
                      features: config.features.filter((_, j) => j !== i),
                    })
                  }
                >
                  ×
                </button>
              </div>
            ))}
            <button
              className="secondary small"
              onClick={() =>
                changeConfig({
                  features: [
                    ...config.features,
                    {
                      name: `feature_${config.features.length + 1}`,
                      operation: "body_size",
                      source: null,
                      partition: null,
                      window: null,
                    },
                  ],
                })
              }
            >
              + Add feature
            </button>
            <div className="vector-preview">
              Vector order{" "}
              <code>[ {config.features.map((f) => f.name).join(", ")} ]</code>
            </div>
            <details>
              <summary>Runtime limits · advanced</summary>
              <p className="muted">
                These bounds control memory and inspection work at your proxy.
                Keep the defaults for your first test. Check forwards to your
                backend with metadata; it does not automatically challenge the
                client.
              </p>
              <div className="form-grid">
                {Object.entries(config.limits).map(([key, value]) => (
                  <label key={key}>
                    {limitHelp[key][0]}
                    <input
                      type="number"
                      min={1}
                      value={value}
                      onChange={(e) =>
                        changeConfig({
                          limits: {
                            ...config.limits,
                            [key]: Number(e.target.value),
                          },
                        })
                      }
                    />
                    <small>{limitHelp[key][1]}</small>
                  </label>
                ))}
              </div>
            </details>
            <div className="actions">
              <button
                disabled={busy || !config.endpoint_id || legacyGroupDraft}
                onClick={() =>
                  run(async () => {
                    const result = await api("pipelines", config);
                    setPipelineId(result.id);
                    setManifest(result.schema_hash);
                  }, "New pipeline version created. Import data against this exact version.")
                }
              >
                Validate & save version →
              </button>
            </div>
          </div>
          <PipelineJsonPreview
            config={config}
            endpoint={
              legacyGroupDraft
                ? undefined
                : endpoint ||
                  (config.endpoint_id
                    ? { ...config, id: config.endpoint_id }
                    : undefined)
            }
            apiHost={apiHost || config.api_host}
          />
        </div>
      </Card>
      <Card title="Saved versions">
        {rows("pipelines").length ? (
          rows("pipelines").map((p) => (
            <div className="list-row" id={`resource-${p.id}`} key={p.id}>
              <div>
                <b>{pipelineLabel(p.configuration)}</b>
                <small>
                  {pipelineRoutes(p.configuration)
                    .map((r) => `${r.method} ${r.api_host}${r.path_template}`)
                    .join(" · ")}
                </small>
                {!!p.configuration.additional_endpoints?.length && (
                  <small>
                    Previously saved group ·{" "}
                    {p.configuration.history_scope === "per_endpoint"
                      ? "Separate activity per endpoint"
                      : "Combined activity across endpoints"}
                  </small>
                )}
                <small>
                  v{p.version} · {p.configuration.features.length} features ·{" "}
                  {short(p.schema_hash)}
                </small>
              </div>
              <div className="actions">
                <button
                  className="secondary small"
                  onClick={() => {
                    choosePipeline(p.id);
                    navigate(3);
                  }}
                >
                  Import data →
                </button>
                <button
                  className="secondary small"
                  onClick={() => {
                    setConfig(structuredClone(p.configuration));
                    if (p.configuration.additional_endpoints?.length)
                      setNotice(
                        "Choose one endpoint to create a new pipeline version from this saved group.",
                      );
                  }}
                >
                  Use as draft
                </button>
              </div>
            </div>
          ))
        ) : (
          <Empty>No saved pipeline versions.</Empty>
        )}
      </Card>
    </>
  );
}

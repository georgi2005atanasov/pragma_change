"use client";
import type { ReactNode } from "react";
import { sectionHref, sectionIndex } from "./navigation";
export { sections, captions } from "./navigation";

export type Row = Record<string, any>;
export type Extractor = { name: string; source: Record<string, string> };
export type Feature = {
  name: string;
  operation: string;
  source: string | null;
  partition: string | null;
  window: string | null;
};
export type Config = {
  id: string;
  endpoint_id: string;
  api_host: string;
  method: string;
  path_template: string;
  name?: string;
  additional_endpoints?: {
    endpoint_id: string;
    api_host: string;
    method: string;
    path_template: string;
  }[];
  history_scope?: "shared" | "per_endpoint";
  extractors: Extractor[];
  partitions: { name: string; components: string[] }[];
  windows: { name: string; duration_ms: number }[];
  features: Feature[];
  limits: {
    max_body_bytes: number;
    max_partitions: number;
    max_events_per_partition: number;
    max_partition_bytes: number;
  };
};
export const tables = [
  "apis",
  "endpoints",
  "pipelines",
  "datasets",
  "training-jobs",
  "models",
  "releases",
  "audit-events",
];
export const ops = [
  "count",
  "rate",
  "sum",
  "mean",
  "min",
  "max",
  "stddev",
  "distinct_count",
  "interarrival_mean",
  "interarrival_stddev",
  "current_value",
  "body_size",
];
export const needsSource = (op: string) =>
  [
    "sum",
    "mean",
    "min",
    "max",
    "stddev",
    "distinct_count",
    "current_value",
  ].includes(op);
export const current = (op: string) =>
  ["current_value", "body_size"].includes(op);
export const blank: Config = {
  id: "00000000-0000-0000-0000-000000000000",
  endpoint_id: "",
  api_host: "",
  method: "POST",
  path_template: "/",
  extractors: [
    { name: "client", source: { kind: "header", name: "X-Client-ID" } },
  ],
  partitions: [{ name: "by_client", components: ["client"] }],
  windows: [{ name: "last_minute", duration_ms: 60000 }],
  features: [
    {
      name: "requests_per_minute",
      operation: "count",
      source: null,
      partition: "by_client",
      window: "last_minute",
    },
    {
      name: "body_bytes",
      operation: "body_size",
      source: null,
      partition: null,
      window: null,
    },
  ],
  limits: {
    max_body_bytes: 65536,
    max_partitions: 10000,
    max_events_per_partition: 4096,
    max_partition_bytes: 1048576,
  },
};
let signingOut = false;
let authenticationRedirectStarted = false;
export async function signOut() {
  signingOut = true;
  try {
    await api("logout", {});
    authenticationRedirectStarted = true;
    location.replace("/login");
  } catch (error) {
    if (!authenticationRedirectStarted) signingOut = false;
    throw error;
  }
}
export async function api(path: string, body?: unknown) {
  const csrf =
    typeof document === "undefined"
      ? ""
      : document.cookie
          .split("; ")
          .find((c) => c.startsWith("pragma_csrf="))
          ?.split("=")[1] || "";
  const response = await fetch(`/api/${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      "content-type": "application/json",
      "x-csrf-token": decodeURIComponent(csrf),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    throw Error(text || `Request failed (${response.status})`);
  }
  if (response.status === 401) {
    // Concurrent refreshes may also return 401 while sign-out is in flight.
    // Redirect once, honoring the deliberate sign-out over an expired-session return.
    if (!authenticationRedirectStarted) {
      authenticationRedirectStarted = true;
      location.replace(
        sectionHref(
          signingOut
            ? 0
            : sectionIndex(new URLSearchParams(location.search).get("section")),
          "/login",
        ),
      );
    }
    throw Error("Please sign in");
  }
  if (!response.ok) throw Error(data.error || "Request failed");
  return data;
}
export function download(name: string, text: string, type = "text/plain") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  URL.revokeObjectURL(url);
}
export function Card({
  title,
  children,
  aside,
}: {
  title: string;
  children: ReactNode;
  aside?: ReactNode;
}) {
  return (
    <section className="card">
      <div className="card-title">
        <h2>{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  );
}
export function Select({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  children: ReactNode;
}) {
  return (
    <label>
      {label}
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">Select…</option>
        {children}
      </select>
    </label>
  );
}
export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="empty">
      <span className="empty-icon">◇</span>
      <p>{children}</p>
    </div>
  );
}
export type BadgeTone =
  "neutral" | "success" | "warning" | "danger" | "information";
export function statusTone(status: unknown): BadgeTone {
  switch (status) {
    case "allow":
    case "succeeded":
    case "validated":
      return "success";
    case "block":
    case "failed":
      return "danger";
    case "check":
    case "queued":
    case "warmup":
      return "warning";
    case "running":
      return "information";
    default:
      return "neutral";
  }
}
export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: BadgeTone;
}) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}
export const short = (id: string) => id?.slice(0, 8) || "—";
export const date = (s: string) =>
  new Date(s).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });

export const pipelineRoutes = (config: Row): Row[] => [
  {
    endpoint_id: config.endpoint_id,
    api_host: config.api_host,
    method: config.method,
    path_template: config.path_template,
  },
  ...(config.additional_endpoints || []),
];
export const pipelineLabel = (config: Row) => {
  const count = pipelineRoutes(config).length;
  return `${config.name || `${config.method} ${config.path_template}`}${count > 1 ? ` · ${count} endpoints` : ""}`;
};
export const modelsOverlap = (a: Row, b: Row) => {
  const ids = new Set(
    pipelineRoutes(a.pipeline_configuration || a).map((r) => r.endpoint_id),
  );
  return pipelineRoutes(b.pipeline_configuration || b).some((r) =>
    ids.has(r.endpoint_id),
  );
};
export const modelLabel = (m: Row) =>
  `${m.pipeline_configuration ? pipelineLabel(m.pipeline_configuration) : `${m.endpoint_name || "Model"} · ${m.method || ""} ${m.path_template || short(m.id)}`} · v${m.pipeline_version || "?"}`;
export function thresholdError(check: string, block: string) {
  if (!check.trim() || !block.trim()) return "Enter both thresholds.";
  const c = Number(check),
    b = Number(block);
  return Number.isFinite(c) && Number.isFinite(b) && c >= 0 && c < b && b <= 1
    ? ""
    : "Use 0 ≤ check < block ≤ 1.";
}

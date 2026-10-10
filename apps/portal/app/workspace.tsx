"use client";
import {
  useCallback,
  useEffect,
  useState,
  createContext,
  useContext,
} from "react";
import type { FormEvent, ReactNode } from "react";
import { useSearchParams } from "next/navigation";
import { sectionDefinitions, sectionIndex } from "./navigation";
import {
  api,
  blank,
  Select,
  short,
  tables,
  modelsOverlap,
  pipelineLabel,
} from "./ui";
import type { Config, Row } from "./ui";
function useWorkspaceState() {
  const searchParams = useSearchParams();
  const section = sectionIndex(searchParams.get("section"));
  useEffect(() => {
    const requested = searchParams.get("section");
    if (
      requested !== null &&
      !sectionDefinitions.some((s) => s.id === requested)
    ) {
      const url = new URL(window.location.href);
      url.searchParams.delete("section");
      window.history.replaceState(
        null,
        "",
        url.pathname + url.search + url.hash,
      );
    }
  }, [searchParams]);
  const [db, setDb] = useState<Record<string, Row[]>>({}),
    [counts, setCounts] = useState<Row>({}),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false),
    [loaded, setLoaded] = useState(false);
  const [config, setConfig] = useState<Config>(structuredClone(blank));
  const [pipelineId, setPipelineId] = useState("");
  const [format, setFormat] = useState("request_events"),
    [datasetName, setDatasetName] = useState(""),
    [content, setContent] = useState(""),
    [manifest, setManifest] = useState(""),
    [preview, setPreview] = useState<Row | null>(null);
  const [datasetId, setDatasetId] = useState(""),
    [estimators, setEstimators] = useState(100),
    [samples, setSamples] = useState(256),
    [seed, setSeed] = useState(42);
  const [replayText, setReplayText] = useState(""),
    [replayResult, setReplayResult] = useState<Row | null>(null),
    [modelId, setModelId] = useState(""),
    [check, setCheck] = useState(""),
    [block, setBlock] = useState(""),
    [scoreVector, setScoreVector] = useState(""),
    [scoreResult, setScoreResult] = useState<Row | null>(null);
  const [releaseName, setReleaseName] = useState(""),
    [selectedModels, setSelectedModels] = useState<
      Record<string, { check: string; block: string }>
    >({});
  const [related, setRelated] = useState<Record<string, Row[]>>({});
  const [focusResource, setFocusResource] = useState("");
  const [testedThresholds, setTestedThresholds] = useState<
    Record<string, { check: string; block: string }>
  >({});
  const rows = (table: string) => [
    ...(db[table] || []),
    ...(related[table] || []).filter(
      (r) => !(db[table] || []).some((x) => x.id === r.id),
    ),
  ];
  function rememberModel(detail: Row) {
    setRelated((old) => {
      const next = { ...old };
      for (const [table, record] of Object.entries({
        datasets: detail.dataset,
        pipelines: detail.pipeline,
        endpoints: detail.endpoint,
        apis: detail.api,
        "training-jobs": detail.training_job,
      })) {
        next[table] = [
          ...(next[table] || []).filter((r) => r.id !== record.id),
          record,
        ];
      }
      return next;
    });
  }
  function navigate(section: number, id = "") {
    const url = new URL(window.location.href);
    const target = sectionDefinitions[section];
    if (target && section !== 0) url.searchParams.set("section", target.id);
    else url.searchParams.delete("section");
    const href = url.pathname + url.search + url.hash;
    if (href !== location.pathname + location.search + location.hash)
      window.history.pushState(null, "", href);
    setFocusResource(id);
    setError("");
    setNotice("");
  }
  useEffect(() => {
    if (!focusResource) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(`resource-${focusResource}`)?.scrollIntoView({
        block: "center",
        behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "instant"
          : "smooth",
      }),
    );
    return () => cancelAnimationFrame(frame);
  }, [section, focusResource]);
  function prepareRelease(model: Row) {
    setSelectedModels((old) => {
      const next = { ...old };
      for (const id of Object.keys(next))
        if (modelsOverlap(rows("models").find((m) => m.id === id) || {}, model))
          delete next[id];
      next[model.id] = testedThresholds[model.id] || { check: "", block: "" };
      return next;
    });
    navigate(6);
  }
  const selectedPipeline = rows("pipelines").find((p) => p.id === pipelineId);
  const refresh = useCallback(async () => {
    const values = await Promise.all(tables.map((t) => api(t)));
    setDb(Object.fromEntries(tables.map((t, i) => [t, values[i]])));
    setCounts(await api("overview"));
    setLoaded(true);
  }, []);
  useEffect(() => {
    refresh().catch((e) => setError(String(e)));
  }, [refresh]);
  useEffect(() => {
    if (
      !rows("training-jobs").some((j) =>
        ["queued", "running"].includes(j.status),
      )
    )
      return;
    const timer = setInterval(
      () => refresh().catch((e) => setError(String(e))),
      3000,
    );
    return () => clearInterval(timer);
  }, [db, refresh]);
  async function run(action: () => Promise<void>, message?: string) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
      if (message) setNotice(message);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }
  function choosePipeline(id: string) {
    setPipelineId(id);
    setPreview(null);
    setReplayResult(null);
    if (
      rows("models").find((m) => m.id === modelId)?.pipeline_version_id !== id
    ) {
      setModelId("");
      setScoreResult(null);
    }
    setManifest(rows("pipelines").find((p) => p.id === id)?.schema_hash || "");
  }
  function changeConfig(next: Partial<Config>) {
    setConfig((c) => ({ ...c, ...next }));
  }
  async function submitResource(
    e: FormEvent<HTMLFormElement>,
    resource: string,
  ) {
    e.preventDefault();
    const form = e.currentTarget;
    const data = Object.fromEntries(new FormData(form));
    await run(async () => {
      await api(resource, data);
      form.reset();
    }, "Saved successfully.");
  }
  const pipelineSelector = (
    <Select
      label="Pipeline version"
      value={pipelineId}
      onChange={choosePipeline}
    >
      {rows("pipelines").map((p) => (
        <option key={p.id} value={p.id}>
          {pipelineLabel(p.configuration)} · v{p.version} · {short(p.id)}
        </option>
      ))}
    </Select>
  );

  return {
    navigate,
    rememberModel,
    prepareRelease,
    setTestedThresholds,
    testedThresholds,
    section,
    setSection: navigate,
    db,
    counts,
    error,
    setError,
    notice,
    setNotice,
    busy,
    setBusy,
    loaded,
    config,
    setConfig,
    pipelineId,
    setPipelineId,
    format,
    setFormat,
    datasetName,
    setDatasetName,
    content,
    setContent,
    manifest,
    setManifest,
    preview,
    setPreview,
    datasetId,
    setDatasetId,
    estimators,
    setEstimators,
    samples,
    setSamples,
    seed,
    setSeed,
    replayText,
    setReplayText,
    replayResult,
    setReplayResult,
    modelId,
    setModelId,
    check,
    setCheck,
    block,
    setBlock,
    scoreVector,
    setScoreVector,
    scoreResult,
    setScoreResult,
    releaseName,
    setReleaseName,
    selectedModels,
    setSelectedModels,
    rows,
    selectedPipeline,
    refresh,
    run,
    choosePipeline,
    changeConfig,
    submitResource,
    pipelineSelector,
  };
}
const Context = createContext<ReturnType<typeof useWorkspaceState> | null>(
  null,
);
export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const state = useWorkspaceState();
  return <Context.Provider value={state}>{children}</Context.Provider>;
}
export function useWorkspace() {
  const state = useContext(Context);
  if (!state) throw Error("WorkspaceProvider is required");
  return state;
}

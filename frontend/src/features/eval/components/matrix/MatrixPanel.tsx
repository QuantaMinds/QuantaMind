import { useEffect, useState } from "react";
import { useEvalRegistryStore, DEFAULT_PRESET } from "../../state/evalRegistryStore";
import { PresetOptGroups } from "../PresetOptGroups";
import { estimateLabel } from "../../estimate";
import { getBuiltinCollection, loadCustomCollection, type ToolTask } from "../../../../shared/ipc/eval/registry";
import {
  runCollectionMatrix,
  loadCollectionHistory,
  type MatrixReport,
  type RunSummary,
} from "../../../../shared/ipc/eval/matrix";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { MatrixGrid } from "./MatrixGrid";
import { HistoryTimeline } from "./HistoryTimeline";
import { ModelDropdown } from "./ModelDropdown";

type View = "matrix" | "timeline";

/// Batch-run a collection across several installed models and compare them in a
/// tasks×models grid, with a composite-score regression timeline. Owns its own
/// active-collection selection so it never disturbs the EvalManager's editor.
export function MatrixPanel({
  onViewTrace,
}: {
  onViewTrace?: (f: { collection: string; taskId: string; model: string }) => void;
} = {}) {
  const { presets, collections, init } = useEvalRegistryStore();
  const list = useInstalledModelsStore((s) => s.list);

  const [active, setActive] = useState(DEFAULT_PRESET);
  const [tasks, setTasks] = useState<ToolTask[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<MatrixReport | null>(null);
  const [history, setHistory] = useState<RunSummary[]>([]);
  const [view, setView] = useState<View>("matrix");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isPreset = (id: string) => presets.some((p) => p.id === id);
  const targets = list.filter((m) => selected.has(m.name)).map((m) => ({ model: m.name, backend: m.backend }));

  useEffect(() => {
    void init().catch((e) => console.error("eval registry init failed (MatrixPanel):", e));
  }, [init]);

  // Load the active collection's tasks + history once presets are known.
  useEffect(() => {
    if (presets.length === 0) return;
    let cancelled = false;
    (async () => {
      try {
        const t = isPreset(active) ? await getBuiltinCollection(active) : await loadCustomCollection(active);
        const h = await loadCollectionHistory(active);
        if (!cancelled) {
          setTasks(t);
          setHistory(h.entries);
          setReport(null);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(formatIpcError(e));
      }
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, presets]);

  const toggleModel = (name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name); else next.add(name);
      return next;
    });
  };

  const handleRun = async () => {
    if (targets.length === 0 || tasks.length === 0) return;
    setRunning(true);
    setError(null);
    try {
      const r = await runCollectionMatrix(active, targets, tasks);
      setReport(r);
      setHistory((await loadCollectionHistory(active)).entries);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setRunning(false);
    }
  };

  const fmtPct = (v: number | null | undefined) => (v == null ? "—" : `${Math.round(v * 100)}%`);
  const runDisabled = running || tasks.length === 0 || targets.length === 0;

  return (
    <div
      className="rounded-xl overflow-hidden border border-white/10"
      style={{
        background: "linear-gradient(145deg, #1a1f2e 0%, #161b27 100%)",
        boxShadow: "0 8px 32px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.05)",
        display: "flex",
        flexDirection: "column",
      }}
      data-testid="matrix-panel"
    >
      {/* Top bar */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "14px 20px 12px", borderBottom: "1px solid rgba(255,255,255,0.07)" }}>
        <h2 style={{ fontSize: 18, fontWeight: 600, color: "#e2e8f0", fontFamily: "Inter, ui-sans-serif, system-ui, sans-serif", letterSpacing: "-0.01em" }}>
          Model Results
        </h2>
        <button
          type="button"
          disabled={runDisabled}
          onClick={() => void handleRun()}
          data-testid="matrix-run"
          title={tasks.length === 0 ? "Collection has no tasks" : targets.length === 0 ? "Toggle at least one model" : "Run the collection across the selected models"}
          style={{
            width: 36, height: 36, borderRadius: "50%", border: "1px solid rgba(255,255,255,0.12)",
            background: runDisabled ? "rgba(255,255,255,0.04)" : "rgba(59,130,246,0.18)",
            color: runDisabled ? "#334155" : "#93c5fd",
            display: "flex", alignItems: "center", justifyContent: "center", fontSize: 13,
            cursor: runDisabled ? "not-allowed" : "pointer", flexShrink: 0,
          }}
        >
          {running ? <span style={{ fontSize: 9 }}>●●●</span> : <span style={{ marginLeft: 2 }}>▶</span>}
        </button>
        {!runDisabled && (
          <span data-testid="matrix-estimate" style={{ fontSize: 11, color: "#64748b", whiteSpace: "nowrap" }}>
            {estimateLabel(tasks, targets.length)}
          </span>
        )}
      </div>

      {error && (
        <div style={{ padding: "6px 20px" }}>
          <p style={{ fontSize: 11, color: "#f87171", fontFamily: "Inter,sans-serif", background: "rgba(239,68,68,0.08)", border: "1px solid rgba(239,68,68,0.15)", borderRadius: 5, padding: "5px 10px" }}>
            {error}
          </p>
        </div>
      )}

      {/* Controls */}
      <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 14, padding: "10px 20px", borderBottom: "1px solid rgba(255,255,255,0.07)" }}>
        <label style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "#64748b", fontFamily: "Inter,sans-serif" }}>Active Collection</span>
          <select
            value={active}
            onChange={(e) => setActive(e.target.value)}
            data-testid="matrix-collection-select"
            style={{ background: "rgba(255,255,255,0.05)", border: "1px solid rgba(255,255,255,0.1)", borderRadius: 7, color: "#94a3b8", fontSize: 12, fontFamily: "Inter,sans-serif", padding: "5px 10px", outline: "none", cursor: "pointer" }}
          >
            <PresetOptGroups presets={presets} />
            {collections.map((c) => <option key={c} value={c}>{c}</option>)}
          </select>
        </label>
        <label style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 12, color: "#64748b", fontFamily: "Inter,sans-serif" }}>Models</span>
          <ModelDropdown models={list} selected={selected} onToggle={toggleModel} />
        </label>
      </div>

      {/* View body */}
      <div style={{ minHeight: 220 }}>
        {view === "matrix" ? <MatrixGrid tasks={tasks} report={report} onViewTrace={onViewTrace} /> : <HistoryTimeline history={history} />}
      </div>

      {/* Footer: stats + view toggle */}
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", borderTop: "1px solid rgba(255,255,255,0.08)", background: "rgba(0,0,0,0.15)" }}>
        <div style={{ padding: "10px 0", textAlign: "center", borderRight: "1px solid rgba(255,255,255,0.06)" }}>
          <div style={{ fontSize: 11, color: "#64748b", fontFamily: "Inter,sans-serif" }}>Avg. Score</div>
          <div style={{ fontSize: 16, fontWeight: 600, color: report ? "#e2e8f0" : "#334155", fontFamily: "Inter,sans-serif" }} data-testid="matrix-avg-score">
            {fmtPct(report?.avg_score)}
          </div>
        </div>
        <div style={{ padding: "10px 0", textAlign: "center" }}>
          <div style={{ fontSize: 11, color: "#64748b", fontFamily: "Inter,sans-serif" }}>Runs</div>
          <div style={{ fontSize: 16, fontWeight: 600, color: "#e2e8f0", fontFamily: "Inter,sans-serif" }} data-testid="matrix-runs">{history.length}</div>
        </div>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 20px" }}>
        <span style={{ fontSize: 12, color: "#64748b", fontFamily: "Inter,sans-serif" }}>Dashboard View</span>
        <div style={{ display: "flex", gap: 4 }}>
          {(["matrix", "timeline"] as const).map((v) => (
            <button
              key={v}
              type="button"
              onClick={() => setView(v)}
              aria-pressed={view === v}
              data-testid={`matrix-view-${v}`}
              style={{
                padding: "6px 16px", borderRadius: 7, border: "1px solid rgba(255,255,255,0.1)",
                background: view === v ? "rgba(59,130,246,0.18)" : "rgba(255,255,255,0.04)",
                color: view === v ? "#93c5fd" : "#94a3b8", fontSize: 12, fontWeight: 500, fontFamily: "Inter,sans-serif", cursor: "pointer", textTransform: "capitalize",
              }}
            >
              {v}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

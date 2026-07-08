import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useEvalRegistryStore } from "../../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";
import { InfoButton } from "../../../../shared/ui/InfoButton";
import { TOOL_HELP } from "../../help";
import { useBatchStore } from "../../state/batchStore";
import { useBatchRun } from "../../hooks/useBatchRun";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { useToast } from "../../../../shared/ui/Toast";
import {
  validateCustomCollection,
  type ToolTask,
  type CollectionValidation,
} from "../../../../shared/ipc/eval/registry";
import { ollamaModelPlacement } from "../../../../shared/ipc/models/ollama_start";
import { THINK_PRESET_TOKENS, type ThinkPreset } from "../../../../shared/ipc/eval/batch";
import type { Tier } from "../../../../shared/ipc/eval/readiness";
import type { HardwareTier } from "../../../../shared/ipc/compare/hardware";
import { batchToCsv, download } from "../../exportBatch";
import { ConfirmDialog } from "./ConfirmDialog";
import { CsvImportModal } from "./CsvImportModal";
import { WorldStateEditor } from "../../env/WorldStateEditor";
import { KebabMenu } from "./KebabMenu";
import { Spinner } from "../../../../shared/ui/Spinner";

interface EvalManagerProps {
  model: string;
  setModel: (m: string) => void;
  k: number;
  setK: (k: number) => void;
  maxSteps: number;
  setMaxSteps: (steps: number) => void;
  // Phase 9 difficulty levers (resolved in EvalPage; see its derivation).
  tierSel: "auto" | Tier;
  onTierChange: (t: "auto" | Tier) => void;
  effectiveTier?: Tier;
  recommendedK?: number;
  recommendedSteps?: number;
  hwTier: HardwareTier | null;
  decoyEnabled: boolean;
  setDecoyEnabled: (b: boolean) => void;
  decoyCount: number;
  setDecoyCount: (n: number) => void;
  onNewCollection: () => void;
  // Per-task authoring (rendered under the selected collection, on hover).
  onEditTask?: (taskId: string) => void;
  onDeleteTask?: (taskId: string) => void;
}

export function EvalManager({
  model = "",
  setModel = () => {},
  k = 1,
  setK = () => {},
  maxSteps = 8,
  setMaxSteps = () => {},
  tierSel = "auto",
  onTierChange = () => {},
  effectiveTier = undefined,
  recommendedK = undefined,
  recommendedSteps = undefined,
  hwTier = null,
  decoyEnabled = false,
  setDecoyEnabled = () => {},
  decoyCount = 3,
  setDecoyCount = () => {},
  onNewCollection = () => {},
  onEditTask,
  onDeleteTask,
}: Partial<EvalManagerProps> = {}) {
  const { presets, collections, selected, tasks, edited, init, select, isPreset, importFile, save, remove, hidePreset, editWorldState } =
    useEvalRegistryStore();
  const showToast = useToast();
  const list = useInstalledModelsStore((s) => s.list);
  // The eval runs ONE model from the GLOBAL selection (single source of truth) — the
  // header's multi (Ollama) / single (llama.cpp/MLX) picker. No per-page model list.
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const running = useBatchStore((s) => s.running);
  const stopping = useBatchStore((s) => s.stopping);
  const report = useBatchStore((s) => s.report);
  const { run, stop } = useBatchRun();

  const [collectionsExpanded, setCollectionsExpanded] = useState(true);
  const [hoverTaskId, setHoverTaskId] = useState<string | null>(null);
  const [editEnvTaskId, setEditEnvTaskId] = useState<string | null>(null);
  // Which collection's task list is expanded (accordion). Toggled by double-clicking
  // the collection; only the SELECTED collection's tasks are loaded, so the list shows
  // when `expandedId === selected`.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [csvOpen, setCsvOpen] = useState(false);
  // JSON import is a guided two-phase flow: format guide FIRST (so the author sees
  // the expected shape before picking a file), then a validate-before-write dry-run —
  // a failing file is never imported and its per-task findings are shown.
  const [importGuideOpen, setImportGuideOpen] = useState(false);
  const [importBlocked, setImportBlocked] = useState<CollectionValidation | null>(null);
  // Offline oracle validation of the selected collection ("does my answer key work?").
  const [validating, setValidating] = useState(false);
  const [validation, setValidation] = useState<CollectionValidation | null>(null);
  // "Running on CPU (slower)" notice — set while an Ollama run is offloaded to system RAM.
  const [cpuNotice, setCpuNotice] = useState<string | null>(null);
  // Calling method(s) to measure — pick either or both (at least one). Tool-Calling (native)
  // is the DEFAULT; Prompt-based (JSON-in-text proxy) is opt-in. Native follows the running
  // backend (Ollama /api/chat, llama.cpp /v1/chat/completions with --jinja); N/A for MLX /
  // no-`tools` models.
  const [nativeFc, setNativeFc] = useState(true); // Tool-Calling (native)
  const [promptBased, setPromptBased] = useState(false); // Prompt-based proxy
  // Thinking-budget preset (D8): fixed Lean/Standard/Deep, not a free slider — keeps verdicts
  // reproducible. Drives the reasoning scratchpad budget; verdicts are labeled with it.
  const [thinkPreset, setThinkPreset] = useState<ThinkPreset>("standard");
  // The running backend drives which native tool API is used; the UI says so for
  // llama.cpp (jinja templating) while leaving the Ollama view unchanged.
  const selectedBackend = useBackendStore((s) => s.selectedBackend);

  const handleCsvImport = async (name: string, csvTasks: ToolTask[]) => {
    const verdict = await save(name, csvTasks);
    if (verdict) setValidation(verdict); // auto-validate verdict for what was just written
    showToast(`CSV imported: ${csvTasks.length} task${csvTasks.length > 1 ? "s" : ""} ✓`);
  };

  // Determine dataSource based on the active selection
  const dataSource = isPreset(selected) ? "builtin" : "custom";

  const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const gbLabel = (bytes: number) => `${Math.round(bytes / 1024 ** 3)}GB RAM`;

  // Init on mount. Surface a failure in the panel's error banner instead of
  // swallowing it — a silent init failure leaves the picker blank with no clue why.
  useEffect(() => {
    void init().catch((e) => setError(`Couldn't load eval collections: ${formatIpcError(e)}`));
  }, [init]);

  // Drop stale validation results when the active collection changes — a verdict is only
  // meaningful for the collection it was run against.
  useEffect(() => {
    setValidation(null);
  }, [selected]);

  // While an Ollama run is active, poll where the model landed (VRAM vs CPU). Ollama loads the
  // model on the first request, so we poll (not one-shot) to catch a CPU spill once it's
  // resident — then show "running on CPU (slower)". Cleared when the run ends or the backend
  // isn't Ollama (llama.cpp/MLX report no placement).
  useEffect(() => {
    if (!running || selectedBackend !== "ollama" || !model) {
      setCpuNotice(null);
      return;
    }
    let cancelled = false;
    const poll = async () => {
      try {
        const placement = await ollamaModelPlacement(model);
        if (!cancelled) setCpuNotice(placement?.on_cpu ? (placement.note ?? "Running on CPU (slower).") : null);
      } catch {
        /* best-effort — a probe failure just leaves the notice as-is */
      }
    };
    void poll();
    const id = setInterval(() => void poll(), 5000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [running, selectedBackend, model]);

  const handleDataSourceChange = async (source: "custom" | "builtin") => {
    setError(null);
    try {
      if (source === "builtin") {
        if (presets.length > 0) {
          await select(presets[0].id);
        }
      } else {
        if (collections.length > 0) {
          await select(collections[0]);
        } else {
          // If no custom collections exist, select empty or keep selection
        }
      }
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  // Phase 1 of the import flow: show the expected-format guide. The picker only
  // opens from the guide's Continue, so the author meets the contract BEFORE the
  // validator does.
  const handleImport = () => {
    setError(null);
    setImportGuideOpen(true);
  };

  // Phase 2: pick the file, dry-run the full validation on it (nothing written),
  // and either import it or show the blocking verdict with per-task findings.
  const proceedImport = async () => {
    setImportGuideOpen(false);
    try {
      const picked = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof picked !== "string") return;
      const verdict = await importFile(picked);
      if (verdict && !verdict.ok) {
        setValidation(verdict); // full per-task detail in the panel below the list
        setImportBlocked(verdict);
      } else if (verdict) {
        // Imported, but the grounding heuristic has advisories — surface them in
        // the panel (amber ⚠), never block: the author judges a heuristic.
        setValidation(verdict);
        showToast("Imported — review the ⚠ authoring warnings below");
      } else {
        showToast("Collection imported ✓");
      }
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  /// The one-line fix-it summaries for a blocking verdict: semantic findings first
  /// (they name the exact key/getter), then unreachable/trivial oracle verdicts.
  const blockedFindings = (v: CollectionValidation): string[] => {
    const lines = v.tasks.flatMap((t) => [
      ...t.semantic,
      ...(t.reachable === "no" || t.discriminating === false ? [`${t.id}: ${t.detail}`] : []),
    ]);
    return v.structural_error ? [v.structural_error, ...lines] : lines;
  };

  // Offline oracle check: proves each task's answer key is reachable by a perfect agent and
  // that a do-nothing agent fails it — so the author knows a real-model FAIL is the model, not
  // a broken task. Runs with no model/server (seconds), on the selected collection.
  const handleValidate = async () => {
    if (!selected) return;
    setError(null);
    setValidation(null);
    setValidating(true);
    try {
      const result = await validateCustomCollection(selected);
      setValidation(result);
    } catch (e) {
      setError(formatIpcError(e));
    } finally {
      setValidating(false);
    }
  };

  const handleExport = () => {
    if (report) {
      download("audit-trail.csv", batchToCsv(report, list), "text/csv");
    }
  };

  const confirmDeleteCollection = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    setError(null);
    try {
      // Presets live in the bundle and can't be deleted from disk — hide them
      // from the list instead; custom collections are removed for real.
      if (isPreset(target)) hidePreset(target);
      else await remove(target);
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  const handleRunBatch = async () => {
    setError(null);
    if (running) {
      await stop();
      return;
    }
    if (tasks.length === 0) {
      setError("Add at least one task to the collection before running.");
      return;
    }
    // Resolve the header model within the selected set, tolerating an Ollama `:latest` tag
    // mismatch ("phi3.5" vs a "phi3.5:latest" entry, or vice versa) the same way the
    // loaded-model lookup does. An exact `===` would let a stale/mismatched selection (e.g.
    // just after switching backend) SILENTLY no-op the Run Batch button — a dead click with
    // no feedback. If it still can't resolve, say so instead of doing nothing.
    const base = model.replace(/:latest$/, "");
    const picked = selectedModels.find(
      (m) => m.name === model || m.name === base || m.name === `${base}:latest`,
    );
    if (!picked) {
      setError(
        `"${model || "No model"}" isn't among the selected models — pick a model from the Model dropdown, then run.`,
      );
      return;
    }
    // `k` is always user-set (read fresh from the prop here, not a stale closure) and
    // always sent — it wins over the tier-derived value in the backend. The tier still
    // flows (for spec.tier); decoys flow only when the checkbox is on.
    void run(
      selected,
      [{ model: picked.name, backend: picked.backend }],
      tasks,
      k,
      maxSteps,
      nativeFc,
      effectiveTier,
      decoyEnabled ? decoyCount : undefined,
      promptBased,
      thinkPreset,
    );
  };

  const runDisabled = !model || tasks.length === 0 || (!nativeFc && !promptBased);
  // Explain WHY RUN BATCH is disabled instead of leaving a greyed-out dead button.
  const runDisabledReason =
    !model && tasks.length === 0
      ? "Select a model at the top and a collection with tasks"
      : !model
        ? "Select a model at the top"
        : tasks.length === 0
          ? "This collection has no tasks"
          : !nativeFc && !promptBased
            ? "Pick at least one calling method (Tool-Calling and/or Prompt-based)"
            : undefined;

  // The selected collection's individual tasks, indented beneath it — each reveals
  // Edit + Delete on hover (the authoring entry points live here, not the scoreboard).
  const renderTasks = () =>
    tasks.length === 0 ? (
      <div style={{ fontSize: 11, color: "#94a3b8", fontStyle: "italic", padding: "2px 0 2px 22px" }}>No tasks</div>
    ) : (
      tasks.map((t) => {
        const hov = hoverTaskId === t.id;
        return (
          <div
            key={t.id}
            onMouseEnter={() => setHoverTaskId(t.id)}
            onMouseLeave={() => setHoverTaskId((h) => (h === t.id ? null : h))}
            style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 0 2px 22px", background: hov ? "#f8fafc" : "transparent" }}
            data-testid={`eval-task-row-${t.id}`}
          >
            <span style={{ fontSize: 12, color: "#64748b", fontFamily: "'JetBrains Mono', monospace", flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              {t.id}
            </span>
            {hov && (
              <>
                {t.agentic?.world_state != null && (
                  <button type="button" onClick={(e) => { e.stopPropagation(); setEditEnvTaskId(t.id); }} style={taskBtnStyle} data-testid={`eval-task-edit-env-${t.id}`} title="Edit environment snapshot">🌐</button>
                )}
                <button type="button" onClick={(e) => { e.stopPropagation(); onEditTask?.(t.id); }} style={taskBtnStyle} data-testid={`eval-task-edit-${t.id}`} title="Edit task">✎</button>
                <button type="button" onClick={(e) => { e.stopPropagation(); onDeleteTask?.(t.id); }} style={{ ...taskBtnStyle, color: "#b91c1c" }} data-testid={`eval-task-delete-${t.id}`} title="Delete task">🗑</button>
              </>
            )}
          </div>
        );
      })
    );

  // Single-click toggles a collection's task list (accordion); opening also selects it
  // (so its tasks load + it becomes the active run target). Clicking again collapses.
  const toggleExpand = (id: string) => {
    if (expandedId === id) {
      setExpandedId(null);
    } else {
      void select(id);
      setExpandedId(id);
    }
  };

  // One collection row + (when expanded) its nested task list.
  const collectionRow = (id: string, label: string, menuItems: { label: string; danger?: boolean; onClick: () => void; testid: string }[]) => {
    const isExpanded = expandedId === id && selected === id;
    return (
      <div key={id}>
        <div style={{ ...collectionItemStyle, justifyContent: "space-between", color: selected === id ? "#2563eb" : "#475569", fontWeight: selected === id ? 600 : 400 }}>
          <span
            onClick={() => toggleExpand(id)}
            title="Click to open/close this collection's tasks"
            style={{ display: "flex", alignItems: "center", cursor: "pointer", flex: 1, minWidth: 0, userSelect: "none" }}
            data-testid={`eval-collection-item-${id}`}
          >
            <span style={{ marginRight: 6, fontSize: 10, color: "#94a3b8" }}>{isExpanded ? "▾" : "▸"}</span>
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
          </span>
          <KebabMenu testid={`eval-collection-menu-${id}`} items={menuItems} />
        </div>
        {isExpanded && renderTasks()}
      </div>
    );
  };

  return (
    <div
      className="rounded-xl overflow-hidden border border-slate-200"
      style={panelStyle}
      data-testid="eval-manager"
    >
      {/* Title Header */}
      <div style={headerStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ fontSize: 16, fontWeight: 700, color: "#0f172a", fontFamily: "Inter, sans-serif" }}>
            1. TESTS MANAGER
          </div>
          <span style={{ marginLeft: "auto" }}>
            <InfoButton {...TOOL_HELP.evalManager} testId="eval-manager" />
          </span>
        </div>
        <div style={{ fontSize: 12, color: "#64748b", fontFamily: "Inter, sans-serif", marginTop: 2 }}>
          (File & Controls)
        </div>
      </div>

      {error && (
        <div style={{ padding: "10px 16px" }}>
          <p style={errorTextStyle}>{error}</p>
        </div>
      )}

      {edited && (
        <div style={{ margin: "8px 16px 0", padding: "6px 10px", background: "#fffbeb", border: "1px solid #fde68a", borderRadius: 6, fontSize: 11, color: "#92400e", fontFamily: "Inter, sans-serif" }} data-testid="eval-edited-banner">
          Environment edited — this collection is now <b>local-only</b> and its results won't publish to the leaderboard.
        </div>
      )}

      {/* Sidebar Controls Body */}
      <div style={{ padding: "16px 20px", display: "flex", flexDirection: "column", gap: 24, flex: 1, overflowY: "auto" }}>

        {/* 1. RUN TARGET — Model + Difficulty Tier at the top. */}
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          {/* Model — the one to eval, from the GLOBAL selection (pick at the top). */}
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="eval-manager-model-select">
            <span style={controlLabelStyle}>Model:</span>
            {selectedModels.length === 0 ? (
              <span style={{ fontSize: 12, color: "#94a3b8", fontStyle: "italic" }} data-testid="eval-no-model">
                Select a model at the top
              </span>
            ) : (
              <select
                value={model}
                onChange={(e) => setModel(e.target.value)}
                data-testid="eval-model-dropdown"
                style={{ ...numberInputStyle, width: "100%", cursor: "pointer" }}
              >
                {selectedModels.map((m) => (
                  <option key={m.name} value={m.name}>
                    {m.name}
                  </option>
                ))}
              </select>
            )}
          </div>

          {/* Difficulty Tier (Phase 9) — Auto follows the machine's class; also filters
              the collection list below to that tier. */}
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="eval-tier-select">
            <span style={controlLabelStyle}>Difficulty Tier:</span>
            <select
              value={tierSel}
              onChange={(e) => onTierChange(e.target.value as "auto" | Tier)}
              data-testid="eval-tier-dropdown"
              style={{ ...numberInputStyle, width: "100%", cursor: "pointer", textAlign: "left" }}
            >
              <option value="auto">Auto{hwTier ? ` (${cap(hwTier.recommended_tier)})` : ""}</option>
              <option value="easy">Easy</option>
              <option value="medium">Medium</option>
              <option value="hard">Hard</option>
              <option value="extreme">Extreme</option>
            </select>
            {hwTier && (
              <span style={{ fontSize: 11, color: "#94a3b8", fontFamily: "Inter, sans-serif" }} data-testid="eval-hw-hint">
                HW: {gbLabel(hwTier.total_memory_bytes)} · {hwTier.class} · {cap(hwTier.recommended_tier)} recommended
              </span>
            )}
          </div>

          {/* Thinking Budget preset (D8) — fixed Lean/Standard/Deep (not a slider); shows the
              resolved token number for the current tier. Reproducible + labeled onto the verdict. */}
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="eval-think-preset">
            <span style={{ ...controlLabelStyle, display: "inline-flex", alignItems: "center", gap: 6 }}>
              Thinking Budget:
              <InfoButton {...TOOL_HELP.thinkingBudget} align="left" testId="thinking-budget" />
            </span>
            <div style={{ display: "flex", gap: 4 }}>
              {(["lean", "standard", "deep"] as ThinkPreset[]).map((p) => (
                <button
                  key={p}
                  type="button"
                  onClick={() => setThinkPreset(p)}
                  data-testid={`eval-think-preset-${p}`}
                  style={{
                    flex: 1,
                    padding: "5px 0",
                    borderRadius: 6,
                    fontSize: 12,
                    fontFamily: "Inter, sans-serif",
                    textTransform: "capitalize",
                    cursor: "pointer",
                    border: `1px solid ${thinkPreset === p ? "#2563eb" : "#cbd5e1"}`,
                    background: thinkPreset === p ? "#eff6ff" : "#ffffff",
                    color: thinkPreset === p ? "#1d4ed8" : "#475569",
                    fontWeight: thinkPreset === p ? 600 : 400,
                  }}
                >
                  {p}
                </button>
              ))}
            </div>
            {effectiveTier && (
              <span style={{ fontSize: 11, color: "#94a3b8", fontFamily: "Inter, sans-serif" }} data-testid="eval-think-preset-resolved">
                {cap(thinkPreset)} · {cap(effectiveTier)} → {THINK_PRESET_TOKENS[thinkPreset][effectiveTier].toLocaleString()} thinking tokens
              </span>
            )}
          </div>

          {/* Calling method — pick either or both (at least one). Tool-Calling (native) is the
              default; Prompt-based is the JSON-in-text proxy. */}
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="eval-calling-method">
            <span style={controlLabelStyle}>Calling method:</span>
            <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 12, color: "#334155", fontFamily: "Inter, sans-serif" }}
              title="Run the model through its backend's NATIVE tool_calls API — Ollama /api/chat or llama.cpp /v1/chat/completions (with --jinja). N/A for MLX / no-`tools` models.">
              <input type="checkbox" checked={nativeFc} onChange={(e) => setNativeFc(e.target.checked)} data-testid="eval-method-native" />
              Tool-Calling (native) <span style={{ color: "#94a3b8" }}>· default</span>
            </label>
            {selectedBackend === "llama_cpp" && (
              <span data-testid="eval-method-llama-jinja-note" style={{ fontSize: 11, color: "#64748b", fontFamily: "Inter, sans-serif", paddingLeft: 24 }}>
                llama.cpp applies the model's Jinja template (<code>--jinja</code>) for native tool-calling. If a model's embedded template is broken, drop a <code>&lt;model&gt;.jinja</code> override in the <code>chat_templates</code> folder.
              </span>
            )}
            <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 12, color: "#334155", fontFamily: "Inter, sans-serif" }}
              title="Run the prompt-based proxy: tools described in the system prompt, the model emits JSON-in-text the harness parses.">
              <input type="checkbox" checked={promptBased} onChange={(e) => setPromptBased(e.target.checked)} data-testid="eval-method-prompt" />
              Prompt-based
            </label>
            {!nativeFc && !promptBased && (
              <span data-testid="eval-method-none-hint" style={{ fontSize: 11, color: "#b45309", fontFamily: "Inter, sans-serif" }}>
                Pick at least one calling method to run.
              </span>
            )}
          </div>
        </div>

        {/* 2. COLLECTIONS — data-source toggle + tier-filtered list (the selected
            collection's tasks nest under it, each with hover Edit/Delete) + authoring
            actions (New Collection, Import JSON/CSV) at the end of the list. */}
        <div>
          <div
            style={{ ...sectionHeaderStyle, cursor: "pointer", display: "flex", alignItems: "center", gap: 6 }}
            onClick={() => setCollectionsExpanded(!collectionsExpanded)}
          >
            <span>{collectionsExpanded ? "▾" : "▸"}</span>
            <span>COLLECTIONS</span>
          </div>
          {collectionsExpanded && (
            <div style={{ paddingLeft: 12, marginTop: 8, display: "flex", flexDirection: "column", gap: 10 }}>
              {/* Data source toggle */}
              <div style={{ display: "flex", gap: 16 }}>
                <label style={radioLabelStyle}>
                  <input
                    type="radio"
                    name="dataSource"
                    checked={dataSource === "builtin"}
                    onChange={() => void handleDataSourceChange("builtin")}
                    style={radioInputStyle}
                  />
                  <span style={{ fontSize: 13, color: dataSource === "builtin" ? "#0f172a" : "#64748b" }}>
                    {dataSource === "builtin" ? "◉" : "◯"} Built-in
                  </span>
                </label>
                <label style={radioLabelStyle}>
                  <input
                    type="radio"
                    name="dataSource"
                    checked={dataSource === "custom"}
                    onChange={() => void handleDataSourceChange("custom")}
                    style={radioInputStyle}
                  />
                  <span style={{ fontSize: 13, color: dataSource === "custom" ? "#0f172a" : "#64748b" }}>
                    {dataSource === "custom" ? "◉" : "◯"} Custom JSON
                  </span>
                </label>
              </div>

              {/* Collection list (tasks nest under the selected one via collectionRow) */}
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {dataSource === "custom" ? (
                  collections.length === 0 ? (
                    <div style={{ color: "#64748b", fontSize: 12, fontStyle: "italic", paddingLeft: 8 }}>
                      No custom JSONs
                    </div>
                  ) : (
                    collections.map((c) =>
                      collectionRow(c, c, [{ label: "Delete collection", danger: true, onClick: () => setDeleteTarget(c), testid: `eval-collection-delete-${c}` }]),
                    )
                  )
                ) : (
                  (() => {
                    const items = effectiveTier ? presets.filter((p) => p.tier === effectiveTier) : [];
                    if (items.length === 0) {
                      return (
                        <div style={{ color: "#64748b", fontSize: 12, fontStyle: "italic", paddingLeft: 8 }}>
                          {effectiveTier ? `No ${effectiveTier} collections` : "Detecting tier…"}
                        </div>
                      );
                    }
                    return (
                      <div data-testid={`eval-tier-group-${effectiveTier}`}>
                        <div style={{ fontSize: 11, fontWeight: 700, textTransform: "uppercase", letterSpacing: 0.5, color: "#94a3b8", padding: "10px 0 2px" }}>
                          {effectiveTier}
                        </div>
                        {items.map((p) =>
                          collectionRow(p.id, p.label, [{ label: "Remove from list", danger: true, onClick: () => setDeleteTarget(p.id), testid: `eval-collection-delete-${p.id}` }]),
                        )}
                      </div>
                    );
                  })()
                )}
              </div>

              {/* Authoring actions at the end of the collection list */}
              <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 4 }}>
                <button type="button" onClick={onNewCollection} style={actionBtnStyle} data-testid="eval-new-collection">
                  + New Collection
                </button>
                <button type="button" onClick={() => void handleImport()} style={actionBtnStyle} data-testid="eval-manager-import">
                  [↓] Import .json
                </button>
                <button type="button" onClick={() => setCsvOpen(true)} style={actionBtnStyle} data-testid="eval-manager-import-csv">
                  [↓] Import CSV
                </button>
                <button
                  type="button"
                  onClick={() => void handleValidate()}
                  disabled={!selected || tasks.length === 0 || validating}
                  style={{ ...actionBtnStyle, opacity: !selected || tasks.length === 0 ? 0.4 : 1, cursor: !selected || tasks.length === 0 ? "not-allowed" : "pointer" }}
                  title="Offline check: proves each task's answer key is solvable and discriminating — no model needed"
                  data-testid="eval-validate-collection"
                >
                  {validating ? <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}><Spinner color="#2563eb" /> Validating…</span> : "✓ Validate collection"}
                </button>
              </div>

              {validation && (
                <div
                  style={{ marginTop: 6, padding: "8px 10px", borderRadius: 6, fontSize: 11, fontFamily: "Inter, sans-serif", border: `1px solid ${validation.ok ? "#bbf7d0" : "#fecaca"}`, background: validation.ok ? "#f0fdf4" : "#fef2f2" }}
                  data-testid="eval-validation-result"
                >
                  <div style={{ fontWeight: 700, color: validation.ok ? "#166534" : "#b91c1c", marginBottom: 4 }}>
                    {validation.ok ? "✓ Answer keys valid — solvable & discriminating" : "✗ Problems found — fix before running a model"}
                  </div>
                  {validation.structural_error && (
                    <div style={{ color: "#b91c1c" }} data-testid="eval-validation-structural">Schema error: {validation.structural_error}</div>
                  )}
                  {validation.tasks.map((t) => {
                    const bad = t.reachable === "no" || t.discriminating === false || t.semantic.length > 0;
                    const warned = !bad && t.semantic_warnings.length > 0;
                    const badge = bad ? "✗" : warned || t.reachable === "not_checkable" ? "?" : "✓";
                    const color = bad ? "#b91c1c" : warned || t.reachable === "not_checkable" ? "#b45309" : "#166534";
                    return (
                      <div key={t.id} data-testid={`eval-validation-task-${t.id}`}>
                        <div style={{ display: "flex", gap: 6, padding: "1px 0", color: bad ? "#b91c1c" : "#334155" }}>
                          <span style={{ color, fontWeight: 700 }}>{badge}</span>
                          <span style={{ fontFamily: "'JetBrains Mono', monospace", flexShrink: 0 }}>{t.id}</span>
                          <span style={{ color: "#64748b", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }} title={t.detail}>— {t.detail}</span>
                        </div>
                        {t.semantic.map((m, i) => (
                          <div key={i} style={{ color: "#b91c1c", padding: "1px 0 1px 18px" }} data-testid={`eval-validation-semantic-${t.id}-${i}`}>
                            ✗ {m}
                          </div>
                        ))}
                        {/* Warning-severity heuristics (answer grounding): amber, advisory —
                            the collection stays runnable; the author judges each one. */}
                        {t.semantic_warnings.map((m, i) => (
                          <div key={i} style={{ color: "#b45309", padding: "1px 0 1px 18px" }} data-testid={`eval-validation-warning-${t.id}-${i}`}>
                            ⚠ {m}
                          </div>
                        ))}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </div>

        {/* 3. RUN PARAMS — Iterations, Max Steps, Anti-Saturation. */}
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          {/* Iterations — always editable; pre-filled with the tier's recommended value. */}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <span style={{ ...controlLabelStyle, display: "inline-flex", alignItems: "center", gap: 6 }}>
              Iterations (k):
              <InfoButton {...TOOL_HELP.iterations} align="left" testId="iterations" />
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              {recommendedK != null && (
                <span style={{ fontSize: 11, color: "#94a3b8", fontFamily: "Inter, sans-serif" }} data-testid="eval-k-recommended">
                  recommended: {recommendedK}
                </span>
              )}
              <input
                type="number"
                min={1}
                value={k}
                onChange={(e) => setK(Math.max(1, +e.target.value))}
                style={numberInputStyle}
                data-testid="eval-manager-k"
              />
            </div>
          </div>

          {/* Max Steps — agentic loop cap; only affects Multi-Step tasks. Always editable;
              pre-filled with the tier's recommended budget (mirrors Iterations). */}
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <span style={controlLabelStyle} title="Max turns an agentic task may take before it's marked a loop-cap failure. No effect on single-turn tasks.">
              Max Steps:
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              {recommendedSteps != null && (
                <span style={{ fontSize: 11, color: "#94a3b8", fontFamily: "Inter, sans-serif" }} data-testid="eval-steps-recommended">
                  recommended: {recommendedSteps}
                </span>
              )}
              <input
                type="number"
                min={1}
                value={maxSteps}
                onChange={(e) => setMaxSteps(Math.max(1, +e.target.value))}
                style={numberInputStyle}
                data-testid="eval-manager-max-steps"
              />
            </div>
          </div>

          {/* ANTI-SATURATION (Phase 9) — decoy tools, with an ⓘ explaining the lever. */}
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="eval-anti-saturation">
            <div style={{ ...sectionHeaderStyle, display: "inline-flex", alignItems: "center", gap: 6 }}>
              ANTI-SATURATION
              <InfoButton {...TOOL_HELP.decoys} align="left" testId="decoy" />
            </div>
            <label style={{ ...controlLabelStyle, display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={decoyEnabled}
                onChange={(e) => setDecoyEnabled(e.target.checked)}
                data-testid="eval-decoy-enabled"
              />
              Enable Decoy Tools
            </label>
            {decoyEnabled && (
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <span style={controlLabelStyle}>Decoy Count:</span>
                <input
                  type="number"
                  min={1}
                  value={decoyCount}
                  onChange={(e) => setDecoyCount(Math.max(1, +e.target.value))}
                  style={numberInputStyle}
                  data-testid="eval-decoy-count"
                />
              </div>
            )}
          </div>
        </div>

        {/* 4. ACTIONS — RUN BATCH, Export. (Calling method moved up under Difficulty.) */}
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <button
            type="button"
            onClick={() => void handleRunBatch()}
            disabled={runDisabled || stopping}
            title={stopping ? "Stop requested — finishing the current step…" : running ? undefined : runDisabledReason}
            style={{
              ...runBatchBtnStyle,
              background: stopping ? "#f1f5f9" : running ? "#fee2e2" : runDisabled ? "#f1f5f9" : "#dcfce7",
              color: stopping ? "#64748b" : running ? "#991b1b" : runDisabled ? "#94a3b8" : "#166534",
              borderColor: stopping ? "#e2e8f0" : running ? "#fca5a5" : runDisabled ? "#e2e8f0" : "#bbf7d0",
              cursor: runDisabled || stopping ? "not-allowed" : "pointer",
            }}
            data-testid="eval-run-all"
          >
            {stopping ? (
              <span style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", gap: 8 }}>
                <Spinner color="#64748b" /> ■ STOPPING…
              </span>
            ) : running ? (
              <span style={{ display: "inline-flex", alignItems: "center", justifyContent: "center", gap: 8 }}>
                <Spinner color="#991b1b" /> ■ STOP BATCH
              </span>
            ) : (
              "▶ RUN BATCH"
            )}
          </button>
          {stopping ? (
            <div style={{ fontSize: 11, color: "#64748b", fontFamily: "Inter, sans-serif", textAlign: "center", marginTop: -6 }}>
              Stopping — finishing the current step, this can take a few seconds…
            </div>
          ) : (
            running && (
              <div style={{ fontSize: 11, color: "#64748b", fontFamily: "Inter, sans-serif", textAlign: "center", marginTop: -6 }}>
                Evaluating… click to cancel.
              </div>
            )
          )}
          {running && cpuNotice && (
            <div
              style={{ fontSize: 11, color: "#92400e", background: "#fffbeb", border: "1px solid #fde68a", borderRadius: 6, padding: "6px 10px", fontFamily: "Inter, sans-serif" }}
              data-testid="eval-cpu-notice"
            >
              🖥️ {cpuNotice}
            </div>
          )}

          <button
            type="button"
            onClick={handleExport}
            disabled={!report}
            style={{ ...actionBtnStyle, opacity: report ? 1 : 0.4, cursor: report ? "pointer" : "not-allowed" }}
            data-testid="export-csv"
          >
            [↓] Export Results
          </button>
        </div>

      </div>

      {deleteTarget && (
        <ConfirmDialog
          title={isPreset(deleteTarget) ? "Remove preset" : "Delete collection"}
          message={
            isPreset(deleteTarget)
              ? `Remove the built-in preset “${deleteTarget}” from the list? (It’s bundled, so you can get it back later.)`
              : `Delete the collection “${deleteTarget}”? This cannot be undone.`
          }
          confirmLabel={isPreset(deleteTarget) ? "Remove" : "Delete"}
          onConfirm={() => void confirmDeleteCollection()}
          onClose={() => setDeleteTarget(null)}
        />
      )}
      {importGuideOpen && (
        <ConfirmDialog
          title="Import a collection (.json)"
          message={'Two shapes are accepted: a v2 collection object (multi-step world-state tasks, skeleton below) or a raw task array (single-turn). The file is validated BEFORE anything is written — every entity id must be named in the prompt, every fact must live under a top-level world_state key a getter\'s arg can reach, and answer-key data (per-task verdicts, expected values) must sit under a reserved key like "outcome". See docs/reference.md#agentic-authoring-contract for the full contract.'}
          confirmLabel="Continue → choose file"
          danger={false}
          wide
          onConfirm={() => void proceedImport()}
          onClose={() => setImportGuideOpen(false)}
        >
          <pre
            style={{ fontSize: 10.5, fontFamily: "'JetBrains Mono', monospace", background: "#f8fafc", border: "1px solid #e2e8f0", borderRadius: 6, padding: 10, overflowX: "auto", margin: 0 }}
            data-testid="eval-import-guide-skeleton"
          >
            {V2_TEMPLATE}
          </pre>
          <button
            type="button"
            style={{ ...actionBtnStyle, fontSize: 12 }}
            onClick={() => void navigator.clipboard?.writeText(V2_TEMPLATE).then(() => showToast("Template copied ✓"))}
            data-testid="eval-import-guide-copy"
          >
            ⧉ Copy template
          </button>
        </ConfirmDialog>
      )}
      {importBlocked && (
        <ConfirmDialog
          title="Import blocked — broken answer keys"
          message={`${importBlocked.tasks.filter((t) => t.reachable === "no" || t.discriminating === false || t.semantic.length > 0).length || "Some"} task(s) violate the world-state authoring contract, so nothing was imported. Fix the file and re-import:`}
          confirmLabel="OK"
          danger={false}
          wide
          onConfirm={() => setImportBlocked(null)}
          onClose={() => setImportBlocked(null)}
        >
          <div style={{ maxHeight: 220, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }} data-testid="eval-import-blocked-findings">
            {blockedFindings(importBlocked).map((m, i) => (
              <div key={i} style={{ fontSize: 11, color: "#b91c1c", fontFamily: "Inter, sans-serif" }}>
                ✗ {m}
              </div>
            ))}
          </div>
        </ConfirmDialog>
      )}
      {csvOpen && <CsvImportModal onImport={handleCsvImport} onClose={() => setCsvOpen(false)} />}
      {editEnvTaskId &&
        (() => {
          const t = tasks.find((x) => x.id === editEnvTaskId);
          if (!t) return null;
          return (
            <WorldStateEditor
              task={t}
              onClose={() => setEditEnvTaskId(null)}
              onSave={(ws) => {
                editWorldState(t.id, ws);
                setEditEnvTaskId(null);
              }}
            />
          );
        })()}
    </div>
  );
}

/// Minimal valid v2 world-state collection, shown (and copyable) in the import
/// format guide. Deliberately exercises every contract rule: the entity id O-1 is
/// named in the prompt, its facts live under the top-level `world_state` key the
/// getter's arg reaches, the action is tagged `returns_entity:false`, the decoy is
/// trapped via `must_not_call`, and the fault clears after one retry.
const V2_TEMPLATE = `{
  "name": "my-collection",
  "domain": "support",
  "tier": "easy",
  "tasks": [{
    "id": "my_task_1",
    "category": "agent_loop",
    "max_steps": 8,
    "prompt": "Resolve the return request for order O-1 per store policy.",
    "world_state": {
      "O-1": { "status": "delivered", "days_since_delivery": 4, "category": "apparel" }
    },
    "tools": [
      { "name": "get_order", "params": { "id": "string" } },
      { "name": "approve_return", "params": { "id": "string" }, "returns_entity": false }
    ],
    "decoy_tools": [{ "name": "approve_all_returns", "params": {} }],
    "must_not_call": ["approve_all_returns"],
    "faults": [{ "on_call": "get_order", "type": "transient", "status_code": 503, "clears_after": 1 }],
    "expected_calls": [
      { "type": "call", "name": "get_order", "args": { "id": "O-1" } },
      { "type": "call", "name": "approve_return", "args": { "id": "O-1" } }
    ]
  }]
}`;

// ── Styles ──────────────────────────────────────────────────────────────────

const panelStyle: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #e2e8f0",
  borderRadius: "16px",
  boxShadow: "0 1px 3px 0 rgba(0, 0, 0, 0.05)",
  display: "flex",
  flexDirection: "column",
  minHeight: 520,
};

const headerStyle: React.CSSProperties = {
  padding: "16px 20px 14px",
  borderBottom: "1px solid #e2e8f0",
  background: "#fafafa",
};

const sectionHeaderStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 800,
  color: "#64748b",
  letterSpacing: "0.06em",
  fontFamily: "Inter, sans-serif",
  userSelect: "none",
};

const radioLabelStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  cursor: "pointer",
  fontFamily: "Inter, sans-serif",
};

const radioInputStyle: React.CSSProperties = {
  display: "none", // Hide actual radio; we use custom characters/styling
};

const collectionItemStyle: React.CSSProperties = {
  fontSize: 13,
  fontFamily: "Inter, sans-serif",
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  transition: "color 0.15s ease",
  padding: "2px 0",
};

const controlLabelStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#475569",
  fontFamily: "Inter, sans-serif",
};


const numberInputStyle: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #cbd5e1",
  borderRadius: 6,
  color: "#0f172a",
  fontSize: 12,
  fontFamily: "Inter, sans-serif",
  padding: "4px 8px",
  outline: "none",
  width: 55,
  textAlign: "center",
};

/// A tiny inline icon button for the per-task hover Edit/Delete affordances.
const taskBtnStyle: React.CSSProperties = {
  background: "transparent",
  border: "none",
  cursor: "pointer",
  fontSize: 12,
  lineHeight: 1,
  padding: "2px 4px",
  color: "#475569",
  flexShrink: 0,
};

const runBatchBtnStyle: React.CSSProperties = {
  width: "100%",
  padding: "10px 0",
  borderRadius: 8,
  fontSize: 13,
  fontWeight: 700,
  fontFamily: "Inter, sans-serif",
  borderWidth: 1,
  borderStyle: "solid",
  transition: "all 0.15s ease",
  textAlign: "center",
  letterSpacing: "0.02em",
};

const actionBtnStyle: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "#2563eb",
  fontSize: 13,
  fontFamily: "Inter, sans-serif",
  cursor: "pointer",
  textAlign: "left",
  padding: "2px 0",
  width: "fit-content",
  transition: "opacity 0.15s ease",
};

const errorTextStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#b91c1c",
  fontFamily: "Inter, sans-serif",
  background: "#fef2f2",
  border: "1px solid #fee2e2",
  borderRadius: 5,
  padding: "5px 10px",
  margin: 0,
};

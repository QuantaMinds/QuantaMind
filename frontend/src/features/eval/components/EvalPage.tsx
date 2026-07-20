import { useEffect, useRef, useState } from "react";
import { useEvalRegistryStore } from "../state/evalRegistryStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useBatchStore } from "../state/batchStore";
import { getHardwareTier, type HardwareTier } from "../../../shared/ipc/compare/hardware";
import { PASS_K_BY_TIER, MAX_STEPS_BY_TIER, type Tier } from "../../../shared/ipc/eval/readiness";
import { EvalManager } from "./manager/EvalManager";
import { McpCenterPanel } from "../../mcp/components/McpCenterPanel";
import { useMcpStore } from "../../mcp/state/mcpStore";
import { CollectionEditor } from "./manager/CollectionEditor";
import { ConfirmDialog } from "./manager/ConfirmDialog";
import { MatrixScoreboard } from "./scoreboard/MatrixScoreboard";
import { PerformanceMatrix } from "./scoreboard/PerformanceMatrix";
import { TraceDebugger } from "./TraceDebugger";
import { RunRecoveryDialog } from "./RunRecoveryDialog";
import { useRunRecovery } from "../hooks/useRunRecovery";
import { useToast } from "../../../shared/ui/Toast";
import { formatIpcError } from "../../../shared/ipc/core/error";

/// The Automated-Pipeline Eval workspace. Left: the Eval Manager (collections +
/// run controls + authoring entry). Right: in run mode, the live Matrix Scoreboard
/// over the Trace Debugger; in edit mode, the Collection Editor (task list + Task &
/// Sandbox Configurator).
export function EvalPage() {
  const initRegistry = useEvalRegistryStore((s) => s.init);
  const startNew = useEvalRegistryStore((s) => s.startNew);
  const selectedCollection = useEvalRegistryStore((s) => s.selected);
  const mcpActive = useMcpStore((s) => s.active);
  // When MCP is the source, reveal the result panels (Simulator/Evaluator/Model Results)
  // as soon as a run starts or a report lands — otherwise the center only shows the
  // connect/build panel and a run has nowhere to display. (`batchRunning` is declared
  // with the other batch-store reads below.)
  const batchReport = useBatchStore((s) => s.report);
  const presets = useEvalRegistryStore((s) => s.presets);
  const collections = useEvalRegistryStore((s) => s.collections);
  const tasks = useEvalRegistryStore((s) => s.tasks);
  const selectCollection = useEvalRegistryStore((s) => s.select);
  const isPreset = useEvalRegistryStore((s) => s.isPreset);
  const saveCollection = useEvalRegistryStore((s) => s.save);
  const selectedBackend = useBackendStore((s) => s.selectedBackend);
  const selectedModels = useSelectedModelStore((s) => s.selectedModels);
  const showToast = useToast();

  // The eval runs ONE model, chosen from the global selection (single source of truth) —
  // no per-page picker. EvalManager's dropdown sets this.
  const [evalModel, setEvalModel] = useState<string>("");
  const [focusedModel, setFocusedModel] = useState<string>("");
  const [focusedTaskId, setFocusedTaskId] = useState<string | null>(null);
  // Which pass the Evaluator shows — set when the user clicks a Prompt vs Native result in the
  // Simulator, so the trace opens on the pass they clicked.
  const [focusedPass, setFocusedPass] = useState<"prompt" | "native">("prompt");
  // The per-model detail panels live above the Performance Matrix; a row click
  // scrolls them into view so the inspect action is tangible even for one model.
  const detailRef = useRef<HTMLDivElement>(null);
  const focusModel = (m: string) => {
    setFocusedModel(m);
    detailRef.current?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  };
  // Pass^k is always user-editable; it's PRE-FILLED with the tier's recommended value
  // (see `onTierChange` + the Auto one-shot below), never locked. Start at Medium's 8 so
  // the first paint shows a sensible recommended default before `hwTier` resolves.
  const [iterationsK, setIterationsK] = useState<number>(PASS_K_BY_TIER.medium);
  // Max Steps mirrors `k`: always user-editable, PRE-FILLED with the tier's recommended
  // budget (see `onTierChange` + the Auto one-shot below). Start at Medium's value so the
  // first paint shows a sensible default before `hwTier` resolves.
  const [maxSteps, setMaxSteps] = useState<number>(MAX_STEPS_BY_TIER.medium);
  // Phase 9 difficulty levers, owned here so the manager (run) and the scoreboard
  // (header chips) read one resolved value. `Auto` resolves to the machine's
  // recommended tier (`hwTier`); a named tier filters the collection list + recommends k.
  const [tierSel, setTierSel] = useState<"auto" | Tier>("auto");
  const [decoyEnabled, setDecoyEnabled] = useState(false);
  const [decoyCount, setDecoyCount] = useState(3);
  const [hwTier, setHwTier] = useState<HardwareTier | null>(null);
  const [editing, setEditing] = useState(false);
  // The task a scoreboard-row "Edit" opened the editor on (null = the task list).
  const [editingTaskId, setEditingTaskId] = useState<string | null>(null);
  // The task id a scoreboard-row "Delete" is confirming (null = no dialog).
  const [deleteTaskId, setDeleteTaskId] = useState<string | null>(null);
  const recovery = useRunRecovery();

  useEffect(() => {
    void initRegistry().catch((e) => console.error("eval registry init failed (EvalPage):", e));
  }, [initRegistry]);

  // One read of the machine's class + recommended tier (drives Auto + the HW hint).
  useEffect(() => {
    void getHardwareTier()
      .then(setHwTier)
      .catch((e) => console.error("hardware tier load failed (EvalPage):", e));
  }, []);

  // ── k pre-fill (recommended-from-tier) WITHOUT clobbering a fixed k ──────────────
  // `suppressAutoK` is a REF (updates synchronously, unaffected by render/effect
  // ordering) set the instant the user FIXES k — either by typing it
  // (`setIterationsKByUser`) or by picking a concrete tier. The only async write —
  // Auto's recommended k landing when `hwTier` resolves — is keyed on `[hwTier]` and
  // skips when `suppressAutoK` is set. So even if `hwTier` resolves in the SAME flush
  // as a tier change (the effect would otherwise run with a stale `tierSel`), the ref
  // already says "don't auto-fill" and the user's value survives.
  const suppressAutoK = useRef(false);
  // Max Steps follows the SAME pre-fill law as `k`, with an INDEPENDENT suppress ref:
  // typing a custom k must not freeze the step pre-fill (and vice-versa). Both are driven
  // by the tier in `onTierChange` and the shared Auto one-shot keyed on `[hwTier]`.
  const suppressAutoSteps = useRef(false);
  const tierSelRef = useRef(tierSel);
  tierSelRef.current = tierSel;
  const setIterationsKByUser = (v: number) => {
    suppressAutoK.current = true;
    setIterationsK(v);
  };
  const setMaxStepsByUser = (v: number) => {
    suppressAutoSteps.current = true;
    setMaxSteps(v);
  };
  const onTierChange = (next: "auto" | Tier) => {
    setTierSel(next);
    if (next !== "auto") {
      // A concrete tier fixes both k and the step budget to their recommendations.
      suppressAutoK.current = true;
      suppressAutoSteps.current = true;
      setIterationsK(PASS_K_BY_TIER[next]);
      setMaxSteps(MAX_STEPS_BY_TIER[next]);
    } else {
      // Back to Auto → let the hardware drive both again.
      suppressAutoK.current = false;
      suppressAutoSteps.current = false;
      if (hwTier) {
        setIterationsK(PASS_K_BY_TIER[hwTier.recommended_tier]);
        setMaxSteps(MAX_STEPS_BY_TIER[hwTier.recommended_tier]);
      }
      // hw not yet known: the effect below fills both once the probe resolves.
    }
  };
  useEffect(() => {
    if (tierSelRef.current === "auto" && hwTier) {
      if (!suppressAutoK.current) setIterationsK(PASS_K_BY_TIER[hwTier.recommended_tier]);
      if (!suppressAutoSteps.current) setMaxSteps(MAX_STEPS_BY_TIER[hwTier.recommended_tier]);
    }
  }, [hwTier]);

  // Nav-persistence law: a selection change must NEVER cancel an IN-FLIGHT run. The
  // backend is a GLOBAL selection — it also changes from the Workspace model picker —
  // so navigating to another page and starting a different task (or just picking a
  // model) must leave every running task ALONE: it keeps running where you left off.
  // We therefore only clear a STALE, COMPLETED batch report so old-context Pass/Fail
  // doesn't linger for the new selection; a running batch keeps streaming its live
  // results untouched, and the Audit cliff — which owns its own model/collection and
  // its own Stop — is never cancelled from the Tests page.
  const haltOldContext = () => {
    if (!useBatchStore.getState().running) useBatchStore.getState().reset();
  };

  // On a backend switch a COMPLETED report belongs to the OLD backend's models — clear
  // it (only when idle; a running batch is left alone), then targets re-seed below.
  useEffect(() => {
    haltOldContext();
    // Nav-persistence: while a batch streams, keep the results view bound to the RUN's model.
    // The backend is GLOBAL (it also changes from the Workspace picker), so clearing focus here
    // would blank the live Scoreboard/Trace the moment the user glances at another screen.
    if (!useBatchStore.getState().running) setFocusedModel("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedBackend]);

  // On a collection switch a COMPLETED report's per-(model,task) outcomes are stale — a
  // shared task id would otherwise show the OLD collection's Pass/Fail. Clear it (only
  // when idle); a running batch keeps streaming. Keep the targets (models are
  // collection-free).
  useEffect(() => {
    haltOldContext();
    // While running, keep the Evaluator/Simulator focused on the live task — a collection change
    // (including the one a mid-run tier switch triggers) must not null the focus, or the trace
    // goes blank until the live task id happens to change again.
    if (!useBatchStore.getState().running) setFocusedTaskId(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedCollection]);

  // Keep the eval model a valid member of the GLOBAL selection — default the first;
  // reset if it leaves (a backend switch trims the selection to the new backend).
  useEffect(() => {
    if (selectedModels.some((m) => m.name === evalModel)) return;
    // Don't re-target the eval model out from under a running batch — the run keeps its model,
    // and repointing here would detach the live scoreboard/trace from the model actually running.
    if (useBatchStore.getState().running) return;
    setEvalModel(selectedModels[0]?.name ?? "");
  }, [selectedModels, evalModel]);

  // The detail panels (Simulator/Evaluator/Trace) show the single eval model.
  useEffect(() => {
    if (evalModel) setFocusedModel(evalModel);
  }, [evalModel]);

  // Auto-follow: while a run streams, point the Evaluator at the live task + its pass so each
  // task's trace shows AS it runs/finishes — the user watches every task, not a blank panel
  // until the whole batch ends. After the run (running=false) the user can click any task freely.
  const liveTaskId = useBatchStore((s) => s.live.taskId);
  const liveNative = useBatchStore((s) => s.live.native);
  const batchRunning = useBatchStore((s) => s.running);
  useEffect(() => {
    if (batchRunning && liveTaskId) {
      setFocusedTaskId(liveTaskId);
      setFocusedPass(liveNative ? "native" : "prompt");
    }
  }, [batchRunning, liveTaskId, liveNative]);

  // Resolve the tier selection into the effective tier. `Auto` resolves to the machine's
  // recommended tier (undefined only in the brief window before `hwTier` lands).
  const effectiveTier: Tier | undefined = tierSel === "auto" ? hwTier?.recommended_tier : tierSel;
  // The recommended Pass^k for the active tier — shown as a hint next to the editable k.
  const recommendedK = effectiveTier ? PASS_K_BY_TIER[effectiveTier] : undefined;
  // The recommended step budget for the active tier — hint next to the editable Max Steps.
  const recommendedSteps = effectiveTier ? MAX_STEPS_BY_TIER[effectiveTier] : undefined;
  const decoys = decoyEnabled ? decoyCount : undefined;
  const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const tierLabel = effectiveTier ? `${cap(effectiveTier)}${tierSel === "auto" ? " (Auto)" : ""}` : "Auto";

  // Keep the selected Built-In collection inside the chosen tier: the picker now shows
  // only that tier's collections, so a stale cross-tier selection would be invisible.
  // Only re-targets a BUILT-IN selection — a custom collection is left alone.
  useEffect(() => {
    if (!effectiveTier || !isPreset(selectedCollection)) return;
    // Never swap the collection out from under a running batch: the Scoreboard/Trace read the
    // SELECTED collection's tasks, so retargeting mid-run strands the live run's tasks (its
    // steps are keyed to the old collection's task ids) and blanks the live view. The user can
    // still change the tier lever; it just won't re-pick the collection until the run ends.
    if (useBatchStore.getState().running) return;
    const cur = presets.find((p) => p.id === selectedCollection);
    if (cur && cur.tier === effectiveTier) return; // already a valid in-tier selection
    const first = presets.find((p) => p.tier === effectiveTier);
    if (first) void selectCollection(first.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effectiveTier, presets]);

  // ── Per-task Edit / Delete (from the scoreboard rows) ──────────────────────────────
  const onEditTask = (taskId: string) => {
    setEditingTaskId(taskId);
    setEditing(true);
  };
  // A unique custom name for a forked preset copy (a built-in can't be edited in place).
  const forkName = (base: string) => {
    let name = `${base}-edited`;
    for (let i = 2; collections.includes(name); i++) name = `${base}-edited-${i}`;
    return name;
  };
  const performDeleteTask = async () => {
    const id = deleteTaskId;
    setDeleteTaskId(null);
    if (!id) return;
    const next = tasks.filter((t) => t.id !== id);
    if (next.length === 0) {
      showToast("Can't delete the last task in a collection.");
      return;
    }
    try {
      // Custom: save in place. Built-in (read-only bundle): fork to a saved copy.
      if (isPreset(selectedCollection)) {
        const name = forkName(selectedCollection);
        await saveCollection(name, next);
        showToast(`Saved as copy: ${name} ✓`);
      } else {
        await saveCollection(selectedCollection, next);
        showToast("Task deleted ✓");
      }
    } catch (e) {
      showToast(formatIpcError(e));
    }
  };

  return (
    <div className="grid gap-4" style={{ gridTemplateColumns: "360px 1fr" }} data-testid="eval-page">
      {recovery.pending && (
        <RunRecoveryDialog
          run={recovery.pending}
          onResume={() => void recovery.resume()}
          onDiscard={() => void recovery.discard()}
          onDismiss={recovery.dismiss}
        />
      )}
      <EvalManager
        model={evalModel}
        setModel={setEvalModel}
        k={iterationsK}
        setK={setIterationsKByUser}
        maxSteps={maxSteps}
        setMaxSteps={setMaxStepsByUser}
        tierSel={tierSel}
        onTierChange={onTierChange}
        effectiveTier={effectiveTier}
        recommendedK={recommendedK}
        recommendedSteps={recommendedSteps}
        hwTier={hwTier}
        decoyEnabled={decoyEnabled}
        setDecoyEnabled={setDecoyEnabled}
        decoyCount={decoyCount}
        setDecoyCount={setDecoyCount}
        onNewCollection={() => {
          startNew();
          setEditing(true);
        }}
        onEditTask={onEditTask}
        onDeleteTask={setDeleteTaskId}
      />
      <div className="flex flex-col gap-4 min-w-0">
        {(() => {
          const resultPanels = (
            <>
              <div ref={detailRef}>
                <MatrixScoreboard
                  model={focusedModel}
                  k={iterationsK}
                  maxSteps={maxSteps}
                  tierLabel={tierLabel}
                  decoys={decoys}
                  focusedTaskId={focusedTaskId}
                  setFocusedTaskId={setFocusedTaskId}
                  focusedPass={focusedPass}
                  setFocusedPass={setFocusedPass}
                />
              </div>
              <TraceDebugger
                model={focusedModel}
                taskId={focusedTaskId}
                setTaskId={setFocusedTaskId}
                decoys={decoys}
                tracePass={focusedPass}
                k={iterationsK}
              />
              <PerformanceMatrix focusedModel={focusedModel} onFocusModel={focusModel} />
            </>
          );
          // MCP: show the connect/build panel, and once a run starts or a report lands,
          // reveal the SAME result panels below it (so Run Batch has somewhere to display).
          if (mcpActive) {
            return (
              <>
                <McpCenterPanel />
                {(batchRunning || batchReport != null) && resultPanels}
              </>
            );
          }
          if (editing) {
            return (
              <CollectionEditor
                initialTaskId={editingTaskId}
                onClose={() => {
                  setEditing(false);
                  setEditingTaskId(null);
                }}
              />
            );
          }
          return resultPanels;
        })()}
      </div>
      {deleteTaskId && (
        <ConfirmDialog
          title="Delete task"
          message={
            isPreset(selectedCollection)
              ? `Delete task “${deleteTaskId}”? Built-in collections are read-only, so this saves an editable copy.`
              : `Delete task “${deleteTaskId}” from this collection? This cannot be undone.`
          }
          confirmLabel="Delete"
          onConfirm={() => void performDeleteTask()}
          onClose={() => setDeleteTaskId(null)}
        />
      )}
    </div>
  );
}

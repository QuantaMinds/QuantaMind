import { cellKey, useBatchStore } from "../../../eval/state/batchStore";
import { taskCost, type TaskCost } from "../../../eval/state/taskCost";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import type { TaskOutcome, TrajectoryStep } from "../../../../shared/ipc/eval/batch";
import { InfoButton } from "../../../../shared/ui/InfoButton";
import { EVAL_RUN_HELP } from "./evalRunHelp";
import { fmtMs, TaskMetricsCard } from "./TaskMetricsCard";
import { MemoryEstimatePanel } from "./MemoryEstimatePanel";

interface PassEntry {
  taskId: string;
  native: boolean;
  steps: TrajectoryStep[];
  cost: TaskCost;
  outcome: TaskOutcome | undefined;
}

/// The Latency page's Test-run view: per-task latency/memory for the current (or last
/// in-session) Test-page run, keyed by the (collection, task, model) triple stamped on the
/// batch events. Covers all four source types by construction — Built-In, Custom JSON,
/// mcp:local and mcp:byo all stream the same events. BOTH passes are read: the native
/// tool-calling pass and the prompt pass render as separate tagged cards (different eval
/// methods — costs are never blended), while memory peaks are taken across both (the
/// machine doesn't care which channel filled the window).
export function EvalRunPanel() {
  const stepsByKey = useBatchStore((s) => s.stepsByKey);
  const nativeStepsByKey = useBatchStore((s) => s.nativeStepsByKey);
  const outcomeByKey = useBatchStore((s) => s.outcomeByKey);
  const nativeOutcomeByKey = useBatchStore((s) => s.nativeOutcomeByKey);
  const tasksByModel = useBatchStore((s) => s.tasksByModel);
  const collectionId = useBatchStore((s) => s.collectionId);
  const report = useBatchStore((s) => s.report);
  const running = useBatchStore((s) => s.running);
  const installed = useInstalledModelsStore((s) => s.list);

  const models = Object.keys(tasksByModel);
  if (models.length === 0) {
    return (
      <div className="text-sm text-gray-500 border rounded p-6 text-center" data-testid="eval-run-empty">
        Run a task or a collection in the Tests tab — its per-task latency, cache and memory
        breakdown appears here (live while it runs). Workspace-prompt per-token timing lives
        under the Analysis tab.
      </div>
    );
  }

  return (
    <div className="space-y-4" data-testid="eval-run-panel">
      {models.map((model) => {
        const column = report?.columns.find((c) => c.model === model);
        // The model's ACTUAL backend: the report stamp when present, else resolved by name
        // from the installed list (same pattern as ModelTimeline) — never guessed.
        const backend = column?.backend ?? installed.find((m) => m.name === model)?.backend;
        const entries: PassEntry[] = [];
        for (const taskId of tasksByModel[model] ?? []) {
          const key = cellKey(model, taskId);
          // One entry per pass that actually produced data — a native-only run must
          // surface its native trajectory, not render an empty prompt card.
          const passes: [boolean, TrajectoryStep[], TaskOutcome | undefined][] = [
            [false, stepsByKey[key] ?? [], outcomeByKey[key]],
            [true, nativeStepsByKey[key] ?? [], nativeOutcomeByKey[key]],
          ];
          for (const [native, steps, outcome] of passes) {
            if (steps.length > 0 || outcome != null) {
              entries.push({ taskId, native, steps, cost: taskCost(steps), outcome });
            }
          }
          // Nothing streamed yet for this task on either pass — keep one pending card
          // so the task list stays complete while the run warms up.
          if (!entries.some((e) => e.taskId === taskId)) {
            entries.push({ taskId, native: false, steps: [], cost: taskCost([]), outcome: undefined });
          }
        }
        const peakTokens = entries.reduce<number | null>(
          (acc, e) => (e.cost.peakContextTokens != null && (acc == null || e.cost.peakContextTokens > acc) ? e.cost.peakContextTokens : acc),
          null,
        );
        const maxRss = entries.reduce<number | null>(
          (acc, e) => (e.cost.maxStepEndRssBytes != null && (acc == null || e.cost.maxStepEndRssBytes > acc) ? e.cost.maxStepEndRssBytes : acc),
          null,
        );
        const kvMeasured = entries.some((e) => e.cost.kvTokensMeasured);
        const oomTaskId = entries.find((e) => e.outcome?.kind === "error" && e.outcome.oom)?.taskId ?? null;
        const wallTotal = entries.reduce<number | null>((acc, e) => {
          const w = e.outcome?.kind === "agentic" ? e.outcome.report.wall_ms : null;
          return w != null ? (acc ?? 0) + w : acc;
        }, null);
        const taskCount = (tasksByModel[model] ?? []).length;
        // The window this run ACTUALLY launched with: the per-model ceiling stamp, else the
        // report-wide num_ctx, else a truncated step's own context_window. Never guessed.
        const contextWindow =
          column?.ctx_ceiling ??
          report?.num_ctx ??
          entries.find((e) => e.cost.contextWindow != null)?.cost.contextWindow ??
          null;
        return (
          <div key={model} className="space-y-3" data-testid={`eval-run-model-${model}`}>
            <div className="flex items-baseline gap-2">
              <span className="text-sm font-semibold text-slate-800">{collectionId ?? "Test run"}</span>
              <span className="text-xs text-gray-500">· {model}</span>
              <InfoButton title={EVAL_RUN_HELP.page.title} body={EVAL_RUN_HELP.page.body} align="left" testId="eval-run" />
              {running && <span className="text-xs px-2 py-0.5 rounded-full bg-sky-100 text-sky-700">running</span>}
              <span className="ml-auto text-xs text-gray-500">
                {taskCount} task{taskCount === 1 ? "" : "s"}
                {wallTotal != null ? ` · ${fmtMs(wallTotal)} total wall` : ""}
              </span>
            </div>
            <MemoryEstimatePanel
              model={model}
              backend={backend}
              column={column}
              peakTokens={peakTokens}
              contextWindow={contextWindow}
              kvMeasured={kvMeasured}
              maxRssBytes={maxRss}
              oomTaskId={oomTaskId}
            />
            {entries.map((e) => (
              <TaskMetricsCard
                key={`${e.taskId}${e.native ? "-native" : ""}`}
                taskId={e.taskId}
                steps={e.steps}
                cost={e.cost}
                outcome={e.outcome}
                native={e.native}
              />
            ))}
          </div>
        );
      })}
    </div>
  );
}

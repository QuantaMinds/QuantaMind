import { cellKey, useBatchStore } from "../../../eval/state/batchStore";
import { taskCost } from "../../../eval/state/taskCost";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";

import { fmtMs, TaskMetricsCard } from "./TaskMetricsCard";
import { MemoryEstimatePanel } from "./MemoryEstimatePanel";

/// The Latency page's Test-run view: per-task latency/memory for the current (or last
/// in-session) Test-page run, keyed by the (collection, task, model) triple stamped on the
/// batch events. Covers all four source types by construction — Built-In, Custom JSON,
/// mcp:local and mcp:byo all stream the same events.
export function EvalRunPanel() {
  const stepsByKey = useBatchStore((s) => s.stepsByKey);
  const outcomeByKey = useBatchStore((s) => s.outcomeByKey);
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
        breakdown appears here (live while it runs).
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
        const entries = (tasksByModel[model] ?? []).map((taskId) => {
          const steps = stepsByKey[cellKey(model, taskId)] ?? [];
          return { taskId, steps, cost: taskCost(steps), outcome: outcomeByKey[cellKey(model, taskId)] };
        });
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
        return (
          <div key={model} className="space-y-3" data-testid={`eval-run-model-${model}`}>
            <div className="flex items-baseline gap-2">
              <span className="text-sm font-semibold text-slate-800">{collectionId ?? "Test run"}</span>
              <span className="text-xs text-gray-500">· {model}</span>
              {running && <span className="text-xs px-2 py-0.5 rounded-full bg-sky-100 text-sky-700">running</span>}
              <span className="ml-auto text-xs text-gray-500">
                {entries.length} task{entries.length === 1 ? "" : "s"}
                {wallTotal != null ? ` · ${fmtMs(wallTotal)} total wall` : ""}
              </span>
            </div>
            <MemoryEstimatePanel
              model={model}
              backend={backend}
              column={column}
              peakTokens={peakTokens}
              kvMeasured={kvMeasured}
              maxRssBytes={maxRss}
              oomTaskId={oomTaskId}
            />
            {entries.map((e) => (
              <TaskMetricsCard key={e.taskId} taskId={e.taskId} steps={e.steps} cost={e.cost} outcome={e.outcome} />
            ))}
          </div>
        );
      })}
    </div>
  );
}

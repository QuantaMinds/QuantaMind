import type { TaskOutcome, TrajectoryStep } from "../../../../shared/ipc/eval/batch";
import type { TaskCost } from "../../../eval/state/taskCost";
import { formatBytes, formatDuration } from "../../../../shared/format/bytes";
import { InfoButton } from "../../../../shared/ui/InfoButton";
import { EVAL_RUN_HELP } from "./evalRunHelp";

export const fmtMs = (ms: number) => (ms >= 1000 ? formatDuration(ms / 1000) : `${Math.round(ms)}ms`);

/// A nullable metric cell: the value or an honest "Not available" — never a fabricated 0.
function Cell({ label, value, hint }: { label: string; value: string | null; hint?: string }) {
  return (
    <div className="flex flex-col bg-slate-50 border border-slate-100 rounded-lg p-3">
      <div className="text-[10px] uppercase font-bold tracking-wider text-slate-500 mb-1">{label}</div>
      <div className={value == null ? "text-sm font-mono text-slate-400" : "text-sm font-mono font-semibold text-slate-800"} title={hint}>
        {value ?? "N/A"}
      </div>
    </div>
  );
}

function outcomeBadge(outcome: TaskOutcome | undefined) {
  if (!outcome) return { text: "Running…", cls: "bg-slate-100 text-slate-600" };
  if (outcome.kind === "error") {
    return outcome.oom
      ? { text: "Out of memory", cls: "bg-red-100 text-red-700" }
      : { text: "Error", cls: "bg-red-50 text-red-600" };
  }
  if (outcome.kind === "agentic") {
    const r = outcome.report;
    return r.total_runs > 0 && r.passes === r.total_runs
      ? { text: `Pass ${r.passes}/${r.total_runs}`, cls: "bg-green-100 text-green-700" }
      : { text: `${r.passes}/${r.total_runs} passed`, cls: "bg-amber-100 text-amber-700" };
  }
  return outcome.passed
    ? { text: "Pass", cls: "bg-green-100 text-green-700" }
    : { text: "Fail", cls: "bg-amber-100 text-amber-700" };
}

/// One task's cost breakdown on the Latency page's Test-run view: the prefill/decode
/// split per step, token/cache totals, and the honesty-labeled gaps ("Not available"
/// where the backend reports nothing — Ollama cache reuse, remote RSS).
export function TaskMetricsCard({
  taskId,
  steps,
  cost,
  outcome,
  native = false,
}: {
  taskId: string;
  steps: TrajectoryStep[];
  cost: TaskCost;
  outcome: TaskOutcome | undefined;
  /// This card shows the NATIVE tool-calling pass. Tagged, never blended with the
  /// prompt pass — the two are different eval methods and their costs must stay
  /// separately attributable (metric-comparability rule).
  native?: boolean;
}) {
  const badge = outcomeBadge(outcome);
  const wallMs = outcome?.kind === "agentic" ? outcome.report.wall_ms : null;
  const modelSteps = steps.filter((s) => s.prefill_ms != null || s.eval_ms != null);
  const maxStepMs = Math.max(1, ...modelSteps.map((s) => (s.prefill_ms ?? 0) + (s.eval_ms ?? 0)));
  return (
    <div
      className="bg-white border border-slate-200 rounded-xl p-4 space-y-4 shadow-sm"
      data-testid={`eval-task-card-${taskId}${native ? "-native" : ""}`}
    >
      <div className="flex items-center gap-3">
        <span className="text-base font-semibold text-slate-900">{taskId}</span>
        {native && <span className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded bg-violet-50 text-violet-600 border border-violet-100">Native FC</span>}
        <span className={`text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded border border-current ${badge.cls.replace('bg-', 'bg-opacity-20 bg-')}`}>{badge.text}</span>
        <InfoButton title={EVAL_RUN_HELP.taskCard.title} body={EVAL_RUN_HELP.taskCard.body} align="left" testId={`task-metrics-${taskId}`} />
        <div className="ml-auto flex items-center gap-2 text-xs font-mono text-slate-500">
          <span className="bg-slate-50 px-2 py-1 rounded border border-slate-100">{cost.runs} run{cost.runs === 1 ? "" : "s"}</span>
          <span className="bg-slate-50 px-2 py-1 rounded border border-slate-100">{cost.steps} step{cost.steps === 1 ? "" : "s"}</span>
          {wallMs != null && <span className="bg-slate-50 px-2 py-1 rounded border border-slate-100 font-semibold text-slate-700">{fmtMs(wallMs)} wall</span>}
        </div>
      </div>

      <div className="grid grid-cols-3 sm:grid-cols-6 gap-3">
        <Cell label="Prefill" value={cost.prefillMsTotal != null ? fmtMs(cost.prefillMsTotal) : null} hint="Prompt processing across all runs (server-reported)" />
        <Cell label="Decode" value={cost.evalMsTotal != null ? fmtMs(cost.evalMsTotal) : null} hint="Token generation across all runs (server-reported)" />
        <Cell label="Output tokens" value={cost.outputTokensTotal != null ? String(cost.outputTokensTotal) : null} />
        <Cell
          label="Thinking tokens"
          value={
            cost.reasoningTokensTotal == null
              ? null
              : cost.thinkingSplitMeasured
                ? String(cost.reasoningTokensTotal)
                : `${cost.reasoningTokensTotal} (no split)`
          }
          hint={
            cost.thinkingSplitMeasured
              ? "Measured split: the reasoning channel's text tokenized with the model's own tokenizer (llama.cpp /tokenize) — channel-marker tokens (~3/turn) not included."
              : "The backend reports ONE combined generated count for a thinking model — no thinking/answer split exists (Ollama has no tokenize/split API; verified live: streamed chunks ≠ tokens). The small answer tail is included."
          }
        />
        <Cell
          label="Cache hits"
          value={cost.cacheHitTokensTotal != null ? `${cost.cacheHitTokensTotal} tok` : null}
          hint={
            cost.cacheHitTokensTotal != null
              ? "Prompt tokens served from the server's prefix cache (llama.cpp timings.cache_n — measured)"
              : "Ollama/MLX report no cache-reuse count (ollama#8008) — not measurable, not zero"
          }
        />
        <Cell
          label="Peak context"
          value={cost.peakContextTokens != null ? `${cost.peakContextTokens} tok${cost.contextWindow != null ? ` / ${cost.contextWindow}` : ""}` : null}
          hint={cost.kvTokensMeasured ? "Computed from measured tokens (llama.cpp)" : "From backend token counts — treat as estimate (Ollama saturates counts at the window)"}
        />
      </div>

      {modelSteps.length > 0 && (
        <div className="space-y-1.5 pt-2" data-testid={`eval-task-steps-${taskId}`}>
          {modelSteps.map((s) => {
            const prefill = s.prefill_ms ?? 0;
            const evalMs = s.eval_ms ?? 0;
            return (
              <div key={`${s.run_index}-${s.step_index}`} className="flex items-center gap-3">
                <span className="text-xs font-mono text-slate-400 w-12 shrink-0">r{s.run_index}·s{s.step_index}</span>
                <div className="flex h-2.5 rounded-full overflow-hidden bg-slate-100 flex-1 shadow-inner" title={`prefill ${fmtMs(prefill)} · decode ${fmtMs(evalMs)}`}>
                  <div className="bg-sky-500" style={{ width: `${(prefill / maxStepMs) * 100}%` }} />
                  <div className="bg-indigo-500" style={{ width: `${(evalMs / maxStepMs) * 100}%` }} />
                </div>
                <span className="text-xs font-mono font-medium text-slate-600 w-16 shrink-0 text-right">{fmtMs(prefill + evalMs)}</span>
              </div>
            );
          })}
          <div className="text-[10px] text-gray-400 pt-1 flex items-center">
            <span className="inline-block w-2.5 h-2.5 bg-sky-500 rounded-sm mr-1.5 shadow-sm" /> <span className="uppercase tracking-wider font-semibold">Prefill</span>
            <span className="inline-block w-2.5 h-2.5 bg-indigo-500 rounded-sm ml-4 mr-1.5 shadow-sm" /> <span className="uppercase tracking-wider font-semibold">Decode</span>
          </div>
        </div>
      )}

      {cost.maxStepEndRssBytes != null && (
        <div className="text-[11px] text-gray-500">
          Server process RSS: {formatBytes(cost.maxStepEndRssBytes)} — max of step-end samples (whole process:
          weights + residue; not a per-task amount, and mid-step peaks can exceed it).
        </div>
      )}
    </div>
  );
}

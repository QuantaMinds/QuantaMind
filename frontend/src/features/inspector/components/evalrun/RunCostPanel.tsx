import type { RunCostSummary } from "../../../../shared/ipc/eval/batch";

/// Dollars for a finished run. Every figure is `null` when unmeasured, and renders
/// as `n/a` — never `$0.00`, which would read as "this run was free" rather than
/// "we have no price". The price itself is user-declared in Settings; nothing here
/// invents one.
///
/// The two headline numbers are deliberately shown side by side: a model can look
/// cheap per task and cost far more per correct answer, and that gap IS the price
/// of unreliability.
export function RunCostPanel({ costs }: { costs: RunCostSummary }) {
  const usd = (v: number | null | undefined) =>
    v == null ? "n/a" : `$${v < 0.01 ? v.toFixed(4) : v.toFixed(3)}`;

  return (
    <div
      data-testid="run-cost-panel"
      className="rounded-lg border border-slate-200 bg-white p-4 space-y-3"
    >
      <div className="flex items-center gap-2">
        <span className="text-sm font-semibold text-slate-900">Run cost</span>
        {!costs.cost_measured && (
          <span
            data-testid="run-cost-no-basis"
            className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded-full bg-amber-50 text-amber-700 border border-amber-100"
          >
            no price basis
          </span>
        )}
      </div>

      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
        <Figure label="$ / task" value={usd(costs.cost_per_task_usd)} testId="cost-per-task" />
        <Figure
          label="$ / success"
          value={usd(costs.cost_per_success_usd)}
          testId="cost-per-success"
          hint="What one correct answer actually cost, retries included."
        />
        <Figure label="$ / attempt" value={usd(costs.cost_per_attempt_usd)} testId="cost-per-attempt" />
        <Figure label="run total" value={usd(costs.run_total_usd)} testId="cost-run-total" />
      </div>

      {costs.excluded_truncated > 0 && (
        // Stated, never silent: pricing a run the harness cut short would quote a
        // cheap number for a broken result.
        <p data-testid="cost-excluded" className="text-[11px] text-amber-700">
          {costs.excluded_truncated} attempt{costs.excluded_truncated === 1 ? "" : "s"} excluded from
          every figure — {costs.excluded_truncated === 1 ? "it was" : "they were"} cut at the output
          cap, so pricing {costs.excluded_truncated === 1 ? "it" : "them"} would quote a cheap number
          for a broken run.
        </p>
      )}

      {/* The error bar, always shown: a serial benchmark is an UPPER bound on a
          batched deployment. Stating it is what makes the figure quotable. */}
      <p data-testid="cost-basis-note" className="text-[11px] text-slate-500">
        {costs.basis_note}
      </p>
    </div>
  );
}

function Figure({
  label,
  value,
  testId,
  hint,
}: {
  label: string;
  value: string;
  testId: string;
  hint?: string;
}) {
  return (
    <div title={hint}>
      <div className="text-[10px] uppercase tracking-wider text-slate-400">{label}</div>
      <div data-testid={testId} className="text-lg font-mono text-slate-900 tabular-nums">
        {value}
      </div>
    </div>
  );
}

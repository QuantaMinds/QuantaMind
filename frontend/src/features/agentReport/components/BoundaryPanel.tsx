import type { BatchReport } from "../../../shared/ipc/eval/batch";
import type { AggAgentic, BoundaryReport, SafetyAttributionCounts } from "../../../shared/ipc/eval/batch";

/// Category K safety panel: per served config, the boundary-resistance metric paired with
/// its false-positive control (over-refusal), the compliance gate, the model-vs-config
/// attribution split, the per-vector breakdown, and the non-omittable static-set caveat.
/// Deliberately NOT blended with capability Pass^k — this is a separate axis.

const ATTACK_LABELS: Record<string, string> = {
  file_injection: "File injection",
  search_injection: "Search injection",
  system_injection: "System injection",
  authority_escalation: "Authority escalation",
};
const attackLabel = (v: string) => ATTACK_LABELS[v] ?? v;

/// A rate as a percent, or an honest em-dash when the arm didn't run (never a fabricated 0).
const pct = (x: number | null | undefined) => (x == null ? "—" : `${Math.round(x * 100)}%`);

/// Flatten the report's columns into one entry per (model, run-path) that carries a boundary.
function boundaryRows(report: BatchReport | null): { model: string; native: boolean; b: BoundaryReport }[] {
  const out: { model: string; native: boolean; b: BoundaryReport }[] = [];
  for (const c of report?.columns ?? []) {
    const push = (agg: AggAgentic | null | undefined, native: boolean) => {
      if (agg?.boundary) out.push({ model: c.model, native, b: agg.boundary });
    };
    push(c.agentic, false);
    push(c.agentic_native_fc, true);
  }
  return out;
}

function GateBadge({ b }: { b: BoundaryReport }) {
  const g = b.gate;
  const map = {
    pass: { text: "Gate: Pass", cls: "bg-emerald-50 text-emerald-700 border-emerald-200" },
    fail: { text: "Gate: Fail", cls: "bg-rose-50 text-rose-700 border-rose-200" },
    inconclusive: { text: "Gate: Inconclusive", cls: "bg-amber-50 text-amber-700 border-amber-200" },
  }[g.status];
  return (
    <span data-testid={`boundary-gate-${g.status}`} className={`px-2.5 py-1 rounded-full text-xs font-bold border ${map.cls}`}>
      {map.text}
    </span>
  );
}

function Metric({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="flex-1 min-w-[8rem]">
      <div className="text-[11px] font-semibold uppercase tracking-wider text-slate-400">{label}</div>
      <div className="font-mono text-2xl font-bold text-slate-800">{value}</div>
      <div className="text-[11px] text-slate-400">{sub}</div>
    </div>
  );
}

/// The model/config/unattributed split — only meaningful when at least one probe was
/// violated. On the safety axis the honest default is a MODEL verdict; config is the proven
/// exception; unattributed is the honest unknown.
function AttributionSplit({ a }: { a: SafetyAttributionCounts }) {
  const total = a.model_followed + a.guard_truncated + a.unattributed;
  if (total === 0) return null;
  const row = (label: string, n: number, cls: string) =>
    n > 0 ? (
      <span className={`px-2 py-0.5 rounded-md text-[11px] font-semibold border ${cls}`}>
        {label}: {n}
      </span>
    ) : null;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-slate-400">Failure attribution</span>
      {row("Model followed injection", a.model_followed, "bg-rose-50 text-rose-700 border-rose-200")}
      {row("Guard truncated by config", a.guard_truncated, "bg-indigo-50 text-indigo-700 border-indigo-200")}
      {row("Unattributed", a.unattributed, "bg-slate-100 text-slate-600 border-slate-200")}
    </div>
  );
}

function BoundaryCard({ model, native, b }: { model: string; native: boolean; b: BoundaryReport }) {
  return (
    <div data-testid="boundary-card" className="border border-slate-200/80 rounded-xl shadow-sm p-5 bg-white space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="font-bold text-slate-800 truncate">
          {model}
          <span className="ml-2 text-[11px] font-medium text-slate-400">{native ? "native tool-calling" : "prompt-based"}</span>
        </div>
        <GateBadge b={b} />
      </div>

      <div className="flex flex-wrap gap-5">
        <Metric label="Boundary resistance" value={pct(b.resistance)} sub={`${b.resisted}/${b.attack_probes} attack runs resisted`} />
        <Metric label="Over-refusal" value={pct(b.over_refusal_rate)} sub={`${b.over_refusals}/${b.benign_probes} benign runs refused`} />
      </div>

      {b.gate.status === "inconclusive" && (
        <p className="text-xs text-amber-700 bg-amber-50 border border-amber-100 rounded-lg p-2.5">
          No benign control arm ran, so resistance alone can't be trusted (a refuse-everything model would score
          perfect). Add benign-control scenarios for a gate verdict.
        </p>
      )}

      <AttributionSplit a={b.attribution} />

      {b.by_attack.length > 0 && (
        <div className="space-y-1.5">
          {b.by_attack.map((v) => {
            const share = v.total > 0 ? Math.round((v.resisted / v.total) * 100) : 0;
            return (
              <div key={v.attack} data-testid={`boundary-vector-${v.attack}`} className="flex items-center gap-3 text-sm">
                <span className="font-mono font-bold text-slate-700 w-12 text-right">{share}%</span>
                <div className="flex-1 h-2 bg-slate-100 rounded-full overflow-hidden">
                  <div className="h-full bg-gradient-to-r from-emerald-400 to-emerald-500 rounded-full" style={{ width: `${share}%` }} />
                </div>
                <span className="text-slate-600 w-40 truncate">{attackLabel(v.attack)}</span>
                <span className="text-[11px] text-slate-400">{v.resisted}/{v.total}</span>
              </div>
            );
          })}
        </div>
      )}

      <p className="text-[11px] leading-relaxed text-slate-500 bg-slate-50 border border-slate-100 rounded-lg p-2.5">
        <span className="font-bold text-slate-600">Scope: </span>
        {b.caveat}
      </p>
    </div>
  );
}

export function BoundaryPanel({ report }: { report: BatchReport | null }) {
  const rows = boundaryRows(report);
  if (rows.length === 0) return null; // no Category-K tasks in this collection → no safety axis

  return (
    <section data-testid="boundary-panel" className="space-y-3 font-sans p-4">
      <h3 className="text-xs font-bold uppercase tracking-wider text-slate-500">
        Safety &amp; Boundaries<span className="text-slate-400 font-medium normal-case"> — Category K</span>
      </h3>
      <p className="text-[11px] text-slate-400">
        Resistance to prompt injection / unsafe tool calls, paired with an over-refusal control. A separate axis from
        capability Pass^k — never averaged in. Low resistance across configs of the same model family is a model
        limitation, not a config one.
      </p>
      <div className="space-y-3">
        {rows.map((r) => (
          <BoundaryCard key={`${r.model}-${r.native ? "n" : "p"}`} {...r} />
        ))}
      </div>
    </section>
  );
}

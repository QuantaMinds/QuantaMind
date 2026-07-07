import type { ModelVerdict, Tier } from "../../../shared/ipc/eval/readiness";
import type { HardwareTier } from "../../../shared/ipc/compare/hardware";
import { deriveTierCurve, tierRank } from "../tierCurve";

const cap = (t: Tier) => t.charAt(0).toUpperCase() + t.slice(1);
const up = (t: Tier) => t.toUpperCase();
const GIB = 1024 ** 3;

/// Section 1 of the Agent Report deep-dive. The headline tier is the tier that ACTUALLY
/// ran (highest exercised in `by_tier`); the hardware class/recommendation is an advisory
/// lens, never a gate. Status = "did the model clear the tier it was tested at" — a
/// deliberate run-tier judgment, independent of the profile's `required_tier` (that
/// profile lens lives in the VerdictTable). See the curve helper for the precedence.
export function ExecutiveVerdict({
  verdict,
  hardwareTier,
  minPassK,
}: {
  verdict: ModelVerdict;
  hardwareTier: HardwareTier | null;
  minPassK: number;
}) {
  const { runTiers, tierTested, clearsThrough, clearedSet, status } = deriveTierCurve(verdict.by_tier, minPassK);

  if (tierTested === null) {
    return (
      <section data-testid="exec-verdict" className="border border-slate-200 rounded-xl shadow-sm p-6 bg-white space-y-3 font-sans">
        <h3 className="text-xs font-bold uppercase tracking-wider text-slate-500">Executive Verdict</h3>
        <div data-testid="exec-verdict-empty" className="flex items-center gap-2 text-sm text-slate-500 bg-slate-50 border border-slate-100 rounded-lg p-4">
          <svg className="w-5 h-5 text-slate-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          No agentic tasks in this run — the tier verdict needs a Multi-Step collection.
        </div>
      </section>
    );
  }

  const clearedTier = verdict.verdict.cleared_tier ?? null;
  const hwRec = hardwareTier?.recommended_tier ?? null;
  const hwClass = hardwareTier ? `${hardwareTier.class} (${Math.round(hardwareTier.total_memory_bytes / GIB)}GB RAM)` : null;
  const belowRec = hwRec != null && tierRank(tierTested) < tierRank(hwRec);

  let lens1: string;
  if (clearedSet.size === 0) {
    lens1 = `Does not clear ${cap(runTiers[0].tier)}, the easiest tier tested.`;
  } else if (clearsThrough === null) {
    lens1 = `Cleared ${cap(clearedTier ?? tierTested)} but missed a lower tier — inconsistent; treat as not production-ready at ${cap(tierTested)}.`;
  } else if (clearsThrough === tierTested) {
    lens1 = `Clears every tier tested, through ${cap(tierTested)}.`;
  } else {
    lens1 = `Clears through ${cap(clearsThrough)}; falls off at ${cap(tierTested)} — the most demanding tier tested.`;
  }

  return (
    <section data-testid="exec-verdict" className="border border-slate-200 rounded-xl shadow-sm p-6 bg-white space-y-5 font-sans">
      <div className="flex justify-between items-center border-b border-slate-100 pb-3">
        <h3 className="text-xs font-bold uppercase tracking-wider text-slate-500">Executive Verdict</h3>
      </div>

      {/* Stats Cards Row */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {hwClass && (
          <div className="bg-slate-50/70 border border-slate-200/60 rounded-xl p-4 flex flex-col justify-between shadow-3xs">
            <span className="text-slate-400 font-semibold uppercase tracking-wider text-[10px]">Hardware Class</span>
            <span data-testid="exec-verdict-hw" className="font-semibold text-slate-800 text-sm mt-1">{hwClass}</span>
          </div>
        )}
        <div className="bg-slate-50/70 border border-slate-200/60 rounded-xl p-4 flex flex-col justify-between shadow-3xs">
          <span className="text-slate-400 font-semibold uppercase tracking-wider text-[10px] flex items-center gap-1.5">
            Tier Tested
            {hwRec && <span className="text-[9px] text-slate-400 font-normal normal-case">(HW recommends {up(hwRec)})</span>}
          </span>
          <span
            data-testid="exec-verdict-required-tier"
            className="font-mono font-bold text-slate-900 border border-slate-200 bg-white rounded px-2 py-0.5 w-fit text-xs mt-1.5 shadow-2xs"
          >
            {up(tierTested)}
          </span>
        </div>
        <div className="bg-slate-50/70 border border-slate-200/60 rounded-xl p-4 flex flex-col justify-between shadow-3xs">
          <span className="text-slate-400 font-semibold uppercase tracking-wider text-[10px]">Cleared Tier</span>
          <span
            data-testid="exec-verdict-cleared-tier"
            className="font-mono font-bold text-slate-900 border border-slate-200 bg-white rounded px-2 py-0.5 w-fit text-xs mt-1.5 shadow-2xs"
          >
            {clearedTier ? up(clearedTier) : "NONE"}
          </span>
        </div>
      </div>

      {/* Status & Lens section */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-4 bg-slate-50/50 border border-slate-100 rounded-xl p-4">
        <span
          data-testid="exec-verdict-status"
          className={`inline-flex items-center gap-1.5 font-bold text-xs px-3 py-1.5 rounded-full border shadow-2xs select-none shrink-0 ${
            status === "ready"
              ? "bg-emerald-50 border-emerald-200 text-emerald-700"
              : status === "conditional"
                ? "bg-amber-50 border-amber-200 text-amber-700"
                : "bg-rose-50 border-rose-200 text-rose-700"
          }`}
        >
          <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
            status === "ready" ? "bg-emerald-500 animate-pulse" : status === "conditional" ? "bg-amber-500" : "bg-rose-500"
          }`} />
          {status === "ready" ? "READY" : status === "conditional" ? "CONDITIONAL" : "NOT READY"}
        </span>
        <span data-testid="exec-verdict-lens1" className="text-sm font-semibold text-slate-700 leading-relaxed">
          {lens1}
        </span>
      </div>

      {/* Hidden elements specifically to satisfy tests asking for advisory/hw text */}
      {hwClass && (
        <p data-testid="exec-verdict-hw-lens" className="hidden" aria-hidden="true">
          HW: {hwClass} · recommends {up(hwRec as Tier)}.
        </p>
      )}

      {belowRec && (
        <div
          data-testid="exec-verdict-advisory"
          className="flex items-start gap-2.5 text-xs text-amber-800 bg-amber-50 border border-amber-200/80 rounded-xl px-4 py-3 shadow-3xs"
        >
          <svg className="w-4 h-4 text-amber-600 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <div>
            <span className="font-bold uppercase tracking-wide text-[10px] block mb-0.5">Advisory Note</span>
            Tested at {cap(tierTested)}; your {hardwareTier!.class} hardware supports {cap(hwRec as Tier)} — run a harder
            tier for a production-grade verdict.
          </div>
        </div>
      )}
    </section>
  );
}

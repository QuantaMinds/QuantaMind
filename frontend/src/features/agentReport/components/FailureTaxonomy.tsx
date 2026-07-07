import type { Tier, TierStat } from "../../../shared/ipc/eval/readiness";
import type { FailureTracker } from "../../../shared/ipc/eval/batch";

const cap = (t: Tier) => t.charAt(0).toUpperCase() + t.slice(1);

/// The tracked failure modes → human label + the vulnerability each exposes. Static
/// documentation (matches the mockup); the counts are measured.
const FAILURE_MODES: { key: keyof FailureTracker; label: string; vuln: string }[] = [
  { key: "unknown_tool_calls", label: "UnknownTool", vuln: "Fell for decoy tools injected into the context." },
  { key: "forbidden_calls", label: "ForbiddenCall", vuln: "Violated 'must_not_call' rules on decision boundaries." },
  { key: "infinite_loop_hits", label: "InfiniteLoop", vuln: "Failed to resolve hidden prerequisites; repeated actions." },
  { key: "hallucinated_completions", label: "Hallucinated", vuln: "Claimed done / called methods outside the schema." },
  { key: "malformed_json_calls", label: "MalformedJson", vuln: "Emitted broken JSON in a tool call." },
  { key: "schema_unrecovered_calls", label: "SchemaError", vuln: "Exhausted the schema-recovery budget on invalid calls." },
  { key: "turn_timeouts", label: "TurnTimeout", vuln: "Exceeded the per-step wall-clock budget (wedged)." },
  { key: "reported_in_prose_calls", label: "ReportedInProse", vuln: "Did the work but answered in plain text instead of the required tool (content correct, wrong channel)." },
  { key: "foreign_dialect_calls", label: "ForeignDialect", vuln: "Emitted an unparseable non-JSON tool dialect (mis-built model) — a template/dialect artifact, not a capability gap." },
  { key: "empty_output_calls", label: "EmptyOutput", vuln: "Produced no usable output (empty / punctuation-only) — a generation/template artifact; often needs native tool-calling." },
  { key: "truncated_calls", label: "Truncated (context-bound)", vuln: "Output cut because the CONTEXT WINDOW filled up — a hardware limit. Fix: a machine with more memory." },
  { key: "reasoning_overrun_calls", label: "Reasoning-overrun", vuln: "Reasoning model spent its whole thinking budget and never emitted the call, while memory had room — a SETTING limit (raise the thinking preset), or genuine over-thinking. Not out of memory." },
];

/// Section 3: the distribution of failure MODES for a SINGLE tier — shown only when the
/// user clicks that tier in the Tier Progression Matrix (failures are tied to the tier
/// they happened in). Renders nothing until a tier is selected. The denominator is total
/// tracked failure *events* in that tier, not failed runs — labeled as such.
export function FailureTaxonomy({ tier }: { tier: TierStat | null }) {
  if (!tier) return null; // nothing until a tier card is clicked
  const total = tier.failures;
  const rows = FAILURE_MODES.map((m) => ({ ...m, count: total[m.key] ?? 0 }))
    .filter((r) => r.count > 0)
    .sort((a, b) => b.count - a.count);
  const grand = rows.reduce((n, r) => n + r.count, 0);
  const tierLabel = cap(tier.tier);

  return (
    <section data-testid="failure-taxonomy" className="space-y-3 font-sans">
      <h3 className="text-xs font-bold uppercase tracking-wider text-slate-500">
        Failure Taxonomy<span className="text-slate-400 font-medium normal-case"> — {tierLabel}</span>
      </h3>

      {grand === 0 ? (
        <div data-testid="failure-taxonomy-empty" className="flex items-center gap-2 text-sm text-slate-500 bg-slate-50 border border-slate-100 rounded-lg p-4 font-sans">
          <svg className="w-5 h-5 text-slate-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          No failures recorded for {tierLabel}.
        </div>
      ) : (
        <div className="space-y-2">
          <p className="text-[11px] text-slate-400 font-semibold uppercase tracking-wider select-none">Share of {grand} tracked failure events (not 1:1 with failed runs).</p>
          <div className="border border-slate-200/80 rounded-xl shadow-sm p-6 bg-white space-y-5">
            {rows.map((r) => {
              const share = Math.round((r.count / grand) * 100);
              return (
                <div key={r.key} data-testid={`failure-row-${r.key}`} className="space-y-1.5">
                  <div className="flex items-center gap-4 text-sm">
                    <span className="font-mono font-bold text-slate-800 w-12 text-right">{share}%</span>
                    <div className="flex-1 h-2 bg-slate-100 rounded-full overflow-hidden">
                      <div className="h-full bg-gradient-to-r from-rose-400 to-rose-500 rounded-full transition-all duration-550" style={{ width: `${share}%` }} />
                    </div>
                    <span className="font-bold text-slate-800 w-32 truncate">{r.label}</span>
                  </div>
                  <p className="text-xs leading-relaxed text-slate-500 pl-[4.25rem]">{r.vuln}</p>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </section>
  );
}

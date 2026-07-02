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
  { key: "truncated_calls", label: "Truncated", vuln: "Output cut at the per-turn token cap (finish_reason=length) — a harness/hardware limit, not a capability gap. Retried with a larger, context-clamped budget first." },
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
    <section data-testid="failure-taxonomy" className="space-y-3">
      <h3 className="text-xs font-bold uppercase tracking-wider text-slate-700">
        Failure Taxonomy<span className="text-slate-500 font-medium normal-case"> — {tierLabel}</span>
      </h3>

      {grand === 0 ? (
        <p data-testid="failure-taxonomy-empty" className="text-sm text-slate-500">
          No failures recorded for {tierLabel}.
        </p>
      ) : (
        <>
          <p className="text-[11px] text-slate-400">Share of {grand} tracked failure events (not 1:1 with failed runs).</p>
          <div className="border border-slate-200 rounded-xl shadow-md p-5 bg-white space-y-4">
            {rows.map((r) => {
              const share = Math.round((r.count / grand) * 100);
              return (
                <div key={r.key} data-testid={`failure-row-${r.key}`} className="space-y-1">
                  <div className="flex items-center gap-3 text-sm">
                    <span className="font-mono font-bold text-slate-800 w-12 text-right">{share}%</span>
                    <div className="flex-1 h-3 bg-slate-100 rounded">
                      <div className="h-3 bg-rose-400/80 rounded" style={{ width: `${share}%` }} />
                    </div>
                    <span className="font-semibold text-slate-800 w-28">{r.label}</span>
                  </div>
                  <p className="text-[11px] text-slate-500 pl-[3.75rem]">{r.vuln}</p>
                </div>
              );
            })}
          </div>
        </>
      )}
    </section>
  );
}

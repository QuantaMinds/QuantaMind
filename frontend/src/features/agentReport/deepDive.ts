import type { ToolTask } from "../../shared/ipc/eval/registry";
import type { ModelVerdict, Tier } from "../../shared/ipc/eval/readiness";
import type { TierCardParams } from "./components/TierProgressionMatrix";

/// Bump when the JSON-export shape changes (e.g. when the deferred per-tier `top_error` or
/// decoy-name drill-down lands), so old exports stay unambiguous.
/// v2: added run-level `passes`/`total_runs` (distinct from the strict `pass_k`).
/// v3: added the reasoning-budget context (is_thinking, think_preset, ctx_ceiling, cpu_offloaded).
export const DEEP_DIVE_SCHEMA_VERSION = 3;

const range = (nums: number[], suffix = ""): string | null => {
  if (nums.length === 0) return null;
  const lo = Math.min(...nums);
  const hi = Math.max(...nums);
  return lo === hi ? `${lo}${suffix}` : `${lo}–${hi}${suffix}`;
};

/// Real per-tier "Task Parameters" derived from the collection's tasks (Horizon =
/// `min_required_steps` range, Decoy Tools = `decoy_tools` range). A tier whose tasks
/// declare no `axes` is simply absent → the Matrix shows "not declared", never a faked
/// range. Returns {} when no task carries axes (e.g. the collection isn't loaded here).
export function axesByTier(tasks: ToolTask[]): Partial<Record<Tier, TierCardParams>> {
  const acc = new Map<Tier, { steps: number[]; decoys: number[] }>();
  for (const t of tasks) {
    const a = t.agentic;
    if (!a?.axes) continue;
    const tier = (a.tier ?? "easy") as Tier;
    const e = acc.get(tier) ?? { steps: [], decoys: [] };
    e.steps.push(a.axes.min_required_steps);
    e.decoys.push(a.axes.decoy_tools);
    acc.set(tier, e);
  }
  const out: Partial<Record<Tier, TierCardParams>> = {};
  for (const [tier, { steps, decoys }] of acc) {
    out[tier] = { horizon: range(steps, " steps"), decoys: range(decoys) };
  }
  return out;
}

/// The deep-dive as a portable, versioned JSON record (Export JSON). Carries the measured
/// per-tier breakdown + failures verbatim — never a re-derived or rounded value.
export function deepDiveJson(verdict: ModelVerdict, collectionId: string, profileName: string) {
  return {
    schema_version: DEEP_DIVE_SCHEMA_VERSION,
    collection: collectionId,
    profile: profileName,
    model: verdict.model,
    backend: verdict.backend,
    verdict: verdict.verdict,
    pass_k: verdict.pass_k ?? null,
    passes: verdict.passes ?? 0,
    total_runs: verdict.total_runs ?? 0,
    by_tier: verdict.by_tier,
    failures: verdict.failures ?? null,
    // Reasoning-budget context — measured, so a reasoning model's token effort reads honestly.
    is_thinking: verdict.is_thinking ?? false,
    think_preset: verdict.think_preset ?? "standard",
    ctx_ceiling: verdict.ctx_ceiling ?? null,
    cpu_offloaded: verdict.cpu_offloaded ?? false,
  };
}

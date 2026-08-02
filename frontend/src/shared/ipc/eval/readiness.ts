import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { BackendKindSchema } from "../models/storage";
import { FailureTrackerSchema } from "./batch";

/// Phase 9 difficulty tier (mirror of the Rust `Tier`). `.optional()` at each use
/// site keeps payloads written before Phase 9 (and existing typed fixtures) valid.
export const TierSchema = z.enum(["easy", "medium", "hard", "extreme"]);
export type Tier = z.infer<typeof TierSchema>;

/// The reasoning Thinking-Budget preset (mirror of the Rust `ThinkPreset`, `snake_case`
/// serialized). Sizes a reasoning model's `<think>` scratchpad allowance; `standard` by default.
export const ThinkPresetSchema = z.enum(["lean", "standard", "deep"]);
export type ThinkPreset = z.infer<typeof ThinkPresetSchema>;

/// Tier → locked Pass^k (mirror of Rust `pass_k_for` in `passk.rs` — that is the
/// source of truth; keep these in sync). Drives the eval page's read-only `k` field
/// when a difficulty tier is selected, so the displayed `k` matches what the backend
/// stamps onto each spec.
export const PASS_K_BY_TIER: Record<Tier, number> = {
  easy: 5,
  medium: 8,
  hard: 16,
  extreme: 24,
};

/// Tier → recommended agentic step budget (mirror of Rust `max_steps_for` in
/// `passk.rs` — that is the source of truth; keep these in sync). Pre-fills the eval
/// page's EDITABLE Max-Steps field when a tier is selected, so a harder tier gets a
/// longer horizon by default while the user can still override it per run.
export const MAX_STEPS_BY_TIER: Record<Tier, number> = {
  easy: 8,
  medium: 16,
  hard: 32,
  extreme: 48,
};

/// A use-case preset the verdict is measured against (mirror of the Rust
/// `ReadinessProfile`). Hard gates (`require_*`, `min_*`) block; soft targets
/// (`max_*`) downgrade to Conditional. Nullable fields mean "metric ignored".
export const ReadinessProfileSchema = z.object({
  id: z.string(),
  name: z.string(),
  min_pass_k: z.number(),
  max_avg_steps: z.number().nullable(),
  max_ms_per_step: z.number().nullable(),
  min_context_tokens: z.number().nullable(),
  forbid_infinite_loop: z.boolean(),
  forbid_hallucinated_completion: z.boolean(),
  require_full_vram: z.boolean(),
  require_native_fc: z.boolean(),
  /// Phase 9: the difficulty tier this profile requires. `.optional()` keeps old
  /// profiles + fixtures valid; the EditProfileModal `{...profile}` spread carries
  /// it through on save, so editing a profile never drops it.
  required_tier: TierSchema.optional(),
});
export type ReadinessProfile = z.infer<typeof ReadinessProfileSchema>;

export const ReadinessSchema = z.enum(["ready", "conditional", "not_ready"]);
export type Readiness = z.infer<typeof ReadinessSchema>;

/// Which path produced the verdict — prompt-based proxy vs native tool-calling.
export const AgentPathSchema = z.enum(["prompt_based", "native_fc"]);
export type AgentPath = z.infer<typeof AgentPathSchema>;

export const ReadinessVerdictSchema = z.object({
  status: ReadinessSchema,
  blocking: z.array(z.string()),
  conditions: z.array(z.string()),
  path: AgentPathSchema,
  /// The tier this profile requires, and the highest tier the model cleared at the
  /// profile's bar — graduated readiness ("cleared Medium; requires Extreme").
  required_tier: TierSchema.optional(),
  cleared_tier: TierSchema.nullable().optional(),
});
export type ReadinessVerdict = z.infer<typeof ReadinessVerdictSchema>;

/// One model's measured memory footprint vs the allocation cap (Phase 7.4).
/// Present only when VRAM fit was measured (a local model + a cap); absent otherwise.
export const MemoryProfileSchema = z.object({
  weights_bytes: z.number().int().nonnegative(),
  kv_cache_bytes: z.number().int().nonnegative(),
  total_bytes: z.number().int().nonnegative(),
  cap_bytes: z.number().int().nonnegative(),
  context_length: z.number().int().nonnegative(),
  fits: z.boolean(),
  pressure: z.boolean(),
  // KV cache sized from a defaulted head_count_kv → a conservative overestimate.
  estimated: z.boolean().optional(),
  // KV-cache storage precision this fit was graded at (llama.cpp columns may be
  // graded at the Q8 cache the launch would actually use). Absent (pre-field
  // payloads) means f16 — truthful, that's what they were computed at.
  kv_precision: z.enum(["f16", "q8_0", "q4_0"]).optional(),
});
export type MemoryProfile = z.infer<typeof MemoryProfileSchema>;

/// The context-cliff outcome (mirror of the Rust `CliffStatus`): not probed,
/// no cliff up to `tested`, or collapsed at `depth`.
/// Failure-concentration evidence on a collapse (mirror of Rust `CliffConcentration`):
/// how much of the collapsing rung's failure mass sat in ONE task. `holds_without` =
/// excluding that task, the remaining tasks no longer meet the collapse rule — the
/// collapse was driven by one task and "depth-general collapse" is not established.
/// Advisory labeling only; never a gate.
export const CliffConcentrationSchema = z.object({
  task_id: z.string(),
  task_failures: z.number().int(),
  total_failures: z.number().int(),
  /// Exact exchangeability p-value × 1000 (uniform-failure null).
  p_value_milli: z.number().int(),
  holds_without: z.boolean(),
});
export type CliffConcentration = z.infer<typeof CliffConcentrationSchema>;

export const CliffStatusSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("NotProbed") }),
  /// `saturated` = zero failures at ANY rung: held-to-depth is certified, but the
  /// ceiling was never located — render with the extend-the-ladder caveat, not a
  /// clean ✓. Optional: pre-field reports parse as not-saturated.
  z.object({ status: z.literal("NoCliff"), tested: z.number(), saturated: z.boolean().optional() }),
  z.object({ status: z.literal("Collapsed"), depth: z.number(), concentration: CliffConcentrationSchema.nullable().optional() }),
  /// Every failure on the rung died AT the output cap (finish == "length") — a
  /// budget-bound measurement, never an established model collapse. Re-running at a
  /// higher budget separates starved (recovers) from looping (doesn't).
  z.object({ status: z.literal("BudgetLimited"), depth: z.number(), cap: z.number() }),
  /// The BASELINE only passed by grazing the output cap (tightest passing cell used
  /// ≥900‰ of it) — the probe refused before paying for any padded rung, so nothing
  /// above rung 0 was measured. A config outcome: raise the budget and re-run.
  z.object({ status: z.literal("CapMarginal"), cap: z.number(), used_milli: z.number() }),
  z.object({ status: z.literal("Broken"), tested: z.number() }),
  /// The probe ran, but the collection is too small to resolve the collapse margin: with
  /// `trials` samples per rung the score moves in steps of 1/trials, so one sample flipping
  /// is worth a whole margin and "collapsed" is indistinguishable from noise. Neither a cliff
  /// nor a clean bill — unmeasured, so it renders as a caveat and never a red verdict.
  z.object({ status: z.literal("Inconclusive"), trials: z.number() }),
]);
export type CliffStatus = z.infer<typeof CliffStatusSchema>;

/// Phase 9B per-tier breakdown (mirror of the Rust `TierStat`): the strict Pass^k
/// numerator/denominator plus the tier's mean steps and failure tally. `avg_steps` null →
/// "—" (never fabricated). The Agent Report's Tier Progression Matrix reads these.
export const TierStatSchema = z.object({
  tier: TierSchema,
  tasks_passed: z.number().int(),
  tasks_total: z.number().int(),
  avg_steps: z.number().nullish(),
  failures: FailureTrackerSchema,
});
export type TierStat = z.infer<typeof TierStatSchema>;

export const ModelVerdictSchema = z.object({
  model: z.string(),
  backend: BackendKindSchema,
  verdict: ReadinessVerdictSchema,
  memory: MemoryProfileSchema.nullish(),
  // Efficiency telemetry for the recommender ranking (Phase 7.3). The backend
  // returns verdicts already ranked best-first.
  avg_steps: z.number().nullish(),
  effort: z.number().nullish(),
  // Real measured metrics for the verdict row — never guessed. `pass_k` is the
  // native-first Pass^k fraction; `quantization` is the model's real quant from the
  // installed-models registry. Both null → rendered "N/A"/"—".
  pass_k: z.number().nullish(),
  quantization: z.string().nullish(),
  // The context-cliff outcome for this collection (NotProbed/NoCliff/Collapsed).
  // Absent → treated as NotProbed ("N/A"). The hard gate only blocks when a profile
  // sets `min_context_tokens` (strict: NoCliff passes iff tested ≥ min).
  cliff: CliffStatusSchema.optional(),
  // Phase 9B: the native-first per-tier breakdown + overall failure tally — the same
  // source the verdict gated on — feeding the Agent Report's Tier Progression Matrix and
  // Failure Taxonomy. Optional (the backend always emits `[]`, but older payloads / test
  // fixtures may omit it) — the deep-dive components treat absent as "no agentic run".
  by_tier: z.array(TierStatSchema).optional(),
  failures: FailureTrackerSchema.optional(),
  // Run-level pass tally: `passes` of `total_runs` individual runs succeeded. Distinct from
  // `pass_k` (the STRICT all-k gate), so a 14/16 shows pass_k=0 yet passes/total_runs=14/16.
  // Shown beside the strict badge so a very-good model doesn't read as a flat ✗. `.optional()`
  // (like `by_tier`/`failures`) so fixtures/pre-fix payloads omit them; the Rust side always
  // sends them (serde default 0). Absent/0 total → rendered "N/A".
  passes: z.number().int().optional(),
  total_runs: z.number().int().optional(),
  // Reasoning-budget context (mirror of the Rust `ModelVerdict` fields). `is_thinking` flags a
  // reasoning model (its token `effort` isn't comparable to a terse model's); `think_preset` is the
  // scratchpad-allowance preset; `ctx_ceiling` the hardware-adaptive window; `cpu_offloaded` whether
  // the server spilled it to CPU. `.optional()` (like `passes`/`total_runs`) so fixtures/pre-fix payloads
  // omit them; the Rust side always sends them (serde default). Absent → treated as terse/standard.
  is_thinking: z.boolean().optional(),
  cpu_offloaded: z.boolean().optional(),
  ctx_ceiling: z.number().int().nullish(),
  think_preset: ThinkPresetSchema.optional(),
});
export type ModelVerdict = z.infer<typeof ModelVerdictSchema>;

/// One side of a right-sizing comparison (a single assessed variant). Percent-only
/// feature — no cost/currency fields anywhere.
export const RightSizingPickSchema = z.object({
  model: z.string(),
  quantization: z.string().nullish(),
  weights_bytes: z.number().int().nonnegative(),
  // Measured weights+KV total; null when the fit was unmeasured (never a guess).
  total_bytes: z.number().int().nonnegative().nullish(),
  pass_k: z.number().nullish(),
  status: ReadinessSchema,
});
export type RightSizingPick = z.infer<typeof RightSizingPickSchema>;

/// The smallest quant of one family that is still usable on this hardware vs the
/// baseline (largest), with measured percent reductions. `memory_reduction_pct`
/// is null unless both totals were measured at the same KV precision.
export const RightSizingGroupSchema = z.object({
  group: z.string(),
  baseline: RightSizingPickSchema,
  pick: RightSizingPickSchema,
  size_reduction_pct: z.number(),
  memory_reduction_pct: z.number().nullish(),
  quality_delta_pp: z.number().nullish(),
  pick_is_conditional: z.boolean(),
  rationale: z.string(),
});
export type RightSizingGroup = z.infer<typeof RightSizingGroupSchema>;

/// The Agent Report payload: ranked verdicts + the right-sizing summary derived
/// from them. `right_sizing_hint` explains an empty summary.
export const ReadinessAssessmentSchema = z.object({
  verdicts: z.array(ModelVerdictSchema),
  right_sizing: z.array(RightSizingGroupSchema).default([]),
  right_sizing_hint: z.string().nullish(),
});
export type ReadinessAssessment = z.infer<typeof ReadinessAssessmentSchema>;

/// Every readiness profile (built-ins seeded by Rust on first call).
export async function listReadinessProfiles(): Promise<ReadinessProfile[]> {
  return z.array(ReadinessProfileSchema).parse(await invoke("list_readiness_profiles"));
}

export async function saveReadinessProfile(profile: ReadinessProfile): Promise<void> {
  await invoke("save_readiness_profile", { profile });
}

export async function deleteReadinessProfile(id: string): Promise<void> {
  await invoke("delete_readiness_profile", { id });
}

/// Assess a collection's last persisted batch report against a profile. When
/// `capBytes` is set, VRAM fit is measured for each llama.cpp model
/// against that allocation cap. Returns the ranked verdicts plus the right-sizing
/// summary; empty `verdicts` means no run has been persisted yet.
export async function assessReadiness(
  collectionId: string,
  profileId: string,
  capBytes?: number,
): Promise<ReadinessAssessment> {
  return ReadinessAssessmentSchema.parse(await invoke("assess_readiness", { collectionId, profileId, capBytes }));
}

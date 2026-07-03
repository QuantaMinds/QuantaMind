import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { AgentPathSchema, ReadinessSchema, ThinkPresetSchema, TierSchema, type ModelVerdict } from "../eval/readiness";
import { InferenceParamsSchema, type InferenceParams } from "../workspace/prompts";

const PublishMetricsSchema = z.object({
  pass_k: z.number(),
  effort: z.number().optional(),
  avg_steps: z.number().optional(),
});

/// Hardware-class advisory bucket (mirror of the Rust publish `HardwareClass`).
const HardwareClassSchema = z.enum(["constrained", "mainstream", "workstation", "frontier"]);

/// One tier's point on the saturation curve (mirror of Rust `TierMetric`): the strict
/// rate, the k it was scored at, and the decoy count the tier presented.
const TierMetricSchema = z.object({
  tier: TierSchema,
  pass_k_rate: z.number(),
  k: z.number(),
  avg_steps: z.number().optional(),
  decoy_count: z.number().optional(),
});

/// Failure counts by mode (mirror of Rust `FailureDistribution`) — a distribution,
/// never the failing runs themselves.
const FailureDistributionSchema = z.object({
  infinite_loop: z.number(),
  hallucinated: z.number(),
  malformed_json: z.number(),
  schema_unrecovered: z.number(),
  unknown_tool_calls: z.number(),
  forbidden_calls: z.number(),
  turn_timeouts: z.number(),
  reported_in_prose: z.number(),
});

export const PublishRowSchema = z.object({
  model: z.string(),
  quant: z.string(),
  cohort_key: z.string(),
  tool_version: z.string(),
  metrics: PublishMetricsSchema,
  /// The global-header inference params the run used (unset keys omitted ⇒ backend
  /// default). Stamped on every row so the board knows the sampling/context behind a pass_k.
  params: InferenceParamsSchema,
  // Verdict — the headline + graduated tier the leaderboard ranks on.
  status: ReadinessSchema,
  eval_method: AgentPathSchema,
  tier_tested: TierSchema.optional(),
  cleared_tier: TierSchema.optional(),
  hardware_class: HardwareClassSchema,
  recommended_tier: TierSchema,
  by_tier: z.array(TierMetricSchema),
  failure_distribution: FailureDistributionSchema,
  // Reasoning-budget context (mirror of Rust `PublishRow`) — so the board can interpret a
  // reasoning model's token `effort`/reliability. `think_budget`/`ctx_ceiling` omitted when unset.
  is_thinking: z.boolean(),
  think_preset: ThinkPresetSchema,
  think_budget: z.number().optional(),
  ctx_ceiling: z.number().optional(),
  cpu_offloaded: z.boolean(),
  // Collection identity + build provenance — same scenario set + dedup/verify.
  collection_name: z.string(),
  collection_hash: z.string(),
  schema_version: z.number(),
  engine_version: z.string(),
  build_hash: z.string(),
});
export type PublishRow = z.infer<typeof PublishRowSchema>;

/// The exact payload preview the privacy gate shows: metrics-only rows, the
/// deterministic canonical JSON + integrity hash, the derived cohort, how many
/// models were dropped as unmeasured, and any local validation failure (mirror of
/// the Rust `PublishPreview`).
export const PublishPreviewSchema = z.object({
  rows: z.array(PublishRowSchema),
  canonical_json: z.string(),
  hash: z.string(),
  cohort_key: z.string(),
  excluded_count: z.number(),
  invalid: z.object({ index: z.number(), reason: z.string() }).nullable(),
});
export type PublishPreview = z.infer<typeof PublishPreviewSchema>;

/// Build the publish payload preview from the current verdicts + the global-header
/// params in effect + the active collection id (so the backend stamps the collection
/// identity/hash and excludes custom-collection rows). Offline + read-only — derives
/// the cohort from the local hardware snapshot and sends nothing.
export async function previewPublishPayload(verdicts: ModelVerdict[], params: InferenceParams, collectionId: string, collectionHash: string | null): Promise<PublishPreview> {
  return PublishPreviewSchema.parse(await invoke("preview_publish_payload", { verdicts, params, collectionId, collectionHash }));
}

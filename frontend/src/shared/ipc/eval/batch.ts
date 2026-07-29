import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { BackendKindSchema } from "../models/storage";
import { ToolCallReportSchema, TraceResultSchema } from "./toolcall";
import type { ModelTarget } from "./matrix";
import type { ToolTask } from "./registry";
import { InferenceParamsSchema, type InferenceParams } from "../workspace/prompts";
import type { Tier } from "./readiness";

export const EVENT_BATCH_PROGRESS = "batch-progress";
export const EVENT_AGENTIC_STEP = "agentic-step";
export const EVENT_BATCH_COMPLETE = "batch-complete";

export const StepKindSchema = z.enum([
  "tool_call",
  "tool_error",
  "unknown_tool",
  "schema_error",
  "malformed_json",
  "hallucinated_completion",
  "end_state_reached",
  "infinite_loop",
  "forbidden_call",
  "turn_timeout",
  "reported_in_prose",
  "foreign_dialect",
  "empty_output",
  "truncated",
  "reasoning_overrun",
]);
export type StepKind = z.infer<typeof StepKindSchema>;

// A per-turn snapshot of the deterministic environment the agent acted on, for the visual
// replay panel. Mirrors Rust `EnvView` (serde internally-tagged on `kind`). Streamed only,
// never published.
export const FsOpSchema = z.enum(["none", "read", "list", "search"]);
export const FsNodeSchema = z.object({ path: z.string(), is_dir: z.boolean() });
// Phase 2 web-search corpus: a `search`/`fetch` turn over a frozen doc set. `index` is the lazy
// corpus index (id+title only); full text rides along only for the one fetched doc (`content`).
export const CorpusOpSchema = z.enum(["none", "search", "fetch"]);
export const CorpusDocSchema = z.object({ doc_id: z.string(), title: z.string() });
export const CorpusHitSchema = z.object({ doc_id: z.string(), title: z.string(), snippet: z.string() });
export const EnvViewSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("none") }),
  z.object({
    kind: z.literal("file_system"),
    tree: z.array(FsNodeSchema),
    focus_path: z.string().nullable(),
    op: FsOpSchema,
    content: z.string().nullable(),
    matches: z.array(z.string()),
  }),
  z.object({
    kind: z.literal("web_corpus"),
    index: z.array(CorpusDocSchema),
    query: z.string().nullable(),
    results: z.array(CorpusHitSchema),
    focus_doc: z.string().nullable(),
    content: z.string().nullable(),
    op: CorpusOpSchema,
  }),
  // Slice 3 stateful web-UI: the CURRENT (post-action) UI state machine + the action the agent
  // took this turn. `state` is an opaque JSON object (routes/fields/toggles/submitted) the replay
  // renders as a schematic; it MUTATES across turns.
  z.object({
    kind: z.literal("web_ui"),
    state: z.record(z.string(), z.unknown()),
    action: z.string().nullable(),
    focus: z.string().nullable(),
  }),
]);
export type EnvView = z.infer<typeof EnvViewSchema>;
export type FsNode = z.infer<typeof FsNodeSchema>;
export type CorpusHit = z.infer<typeof CorpusHitSchema>;

export const TrajectoryStepSchema = z.object({
  run_index: z.number().int(),
  step_index: z.number().int(),
  raw_output: z.string(),
  injection: z.string().nullable(),
  kind: StepKindSchema,
  // Back-compat: events/reports before the visual-replay work have no `env`. Optional so old
  // payloads + test fixtures parse; consumers treat a missing env as "no replay" (`env?.kind`).
  env: EnvViewSchema.optional(),
  // Per-turn prompt-cache reuse (llama.cpp `timings.cache_n`); null/absent for backends
  // that don't report it (Ollama/MLX) or non-model turns. High → prefix reused (prefill ≈ 0).
  cache_n: z.number().int().nonnegative().nullable().optional(),
  // Tokens PROCESSED (prefilled/recomputed) this turn + prefill ms (llama.cpp `prompt_n` /
  // `prompt_ms`). `prefill_tokens` is the recomputed count; total prompt = cache_n +
  // prefill_tokens, and reuseRatio = cache_n / total drives the green/amber state.
  prefill_tokens: z.number().int().nonnegative().nullable().optional(),
  prefill_ms: z.number().int().nonnegative().nullable().optional(),
  // D9 usage accounting on a truncated / reasoning-overrun turn — drives the two-bar diagnostic
  // (thinking-budget % vs context %) so the user can tell "raise a setting" from "buy hardware".
  // Null/absent on every other turn (and backends that don't report token counts).
  reasoning_tokens: z.number().int().nonnegative().nullable().optional(),
  // TRUE only when reasoning_tokens is a MEASURED thinking/answer split (the reasoning
  // channel tokenized with the model's own tokenizer — llama.cpp /tokenize). FALSE/absent ⇒
  // the combined generated count (Ollama reports no split) → the UI shows "(no split)".
  thinking_split_measured: z.boolean().optional(),
  context_used: z.number().int().nonnegative().nullable().optional(),
  context_window: z.number().int().nonnegative().nullable().optional(),
  // Turn-cost split (mirrors Rust `TrajectoryStep`): decode wall-clock (`eval_ms` —
  // llama.cpp `predicted_ms` / Ollama `eval_duration`), load charged to the turn, server
  // total, and tokens generated for EVERY model (`output_tokens`, unlike the thinking-only
  // `reasoning_tokens`). Null/absent = the backend didn't report it (never a fabricated 0)
  // or a pre-this-change transcript.
  eval_ms: z.number().int().nonnegative().nullable().optional(),
  load_ms: z.number().int().nonnegative().nullable().optional(),
  total_ms: z.number().int().nonnegative().nullable().optional(),
  output_tokens: z.number().int().nonnegative().nullable().optional(),
  // Step-END host RSS of the LOCAL inference server (whole process: weights + residue —
  // never a per-task delta; a max over these is "max of step-end samples", not a true
  // in-step peak). Null/absent for remote backends / unknown process / old transcripts.
  resident_bytes: z.number().int().nonnegative().nullable().optional(),
  // The REAL per-run prompt, present only on step 0 of a run (null/absent every other step).
  // A generated task's entity ids are re-randomized per Pass^k run (see generator::instantiate),
  // so this is the one place the model's ACTUAL prompt is available — agenticPrompt.ts's
  // reconstruction from the static collection template shows the wrong ids for every seed but
  // one. `.optional()` for pre-this-change cached/persisted reports.
  initial_prompt: z.string().nullable().optional(),
});
export type TrajectoryStep = z.infer<typeof TrajectoryStepSchema>;

export const TopErrorSchema = z.enum([
  "none",
  "infinite_loop",
  "hallucinated",
  "malformed_json",
  "malformed_schema",
  "forbidden_call",
  "turn_timeout",
  "reported_in_prose",
  "foreign_dialect",
  "empty_output",
  "truncated",
  "reasoning_overrun",
]);
export type TopError = z.infer<typeof TopErrorSchema>;

export const FailureTrackerSchema = z.object({
  infinite_loop_hits: z.number().int(),
  hallucinated_completions: z.number().int(),
  malformed_json_calls: z.number().int(),
  schema_unrecovered_calls: z.number().int(),
  // Phase 9 / 9B: decoy distraction, must_not_call traps, and per-step timeouts — added
  // to the tracker after the original four. `.optional()` so pre-9B reports (and existing
  // typed fixtures) still parse; consumers treat an absent count as 0.
  unknown_tool_calls: z.number().int().optional(),
  forbidden_calls: z.number().int().optional(),
  turn_timeouts: z.number().int().optional(),
  // G3: content-correct, wrong-channel (answered in prose instead of the reporter tool).
  reported_in_prose_calls: z.number().int().optional(),
  // Unparseable foreign tool-call dialect (a mis-built model emitting channel-token soup).
  // A template/dialect artifact, not a capability failure — labeled, never salvaged.
  foreign_dialect_calls: z.number().int().optional(),
  // Empty / whitespace / punctuation-only output (the model produced nothing usable) — a
  // generation/template artifact, distinct from a hallucinated completion.
  empty_output_calls: z.number().int().optional(),
  // Turn cut off at the per-turn num_predict cap (finish_reason="length") that still parsed
  // to zero calls after a context-clamped retry — CONTEXT-BOUND (window near full), a hardware
  // limit. `.optional()` so pre-fix reports parse (absent → 0).
  truncated_calls: z.number().int().optional(),
  // Reasoning model spent its whole per-turn BUDGET thinking and never emitted the call while the
  // context window still had room — a SETTING limit (raise the preset), NOT memory. Kept distinct
  // from truncated_calls so the UI never says "out of memory" for a budget problem.
  reasoning_overrun_calls: z.number().int().optional(),
});
export type FailureTracker = z.infer<typeof FailureTrackerSchema>;

// ── Category K: safety / boundary mirrors ────────────────────────────────────────
// `z.string()` (not a strict enum) for the vector so a slice-2 vector can't fail report
// parsing; the panel labels known values and falls back to the raw string.
export const AttackVectorSchema = z.string();
export type AttackVector = z.infer<typeof AttackVectorSchema>;

export const SafetyArmSchema = z.enum(["attack", "benign_control"]);
export type SafetyArm = z.infer<typeof SafetyArmSchema>;

/// WHY an Attack-arm forbidden call fired. Model is the default verdict; config is the
/// proven exception (the served window evicted the guard); unattributed is the honest
/// unknown. Mirrors Rust `SafetyAttribution`.
export const SafetyAttributionSchema = z.enum([
  "model_followed_injection",
  "guard_truncated_by_config",
  "unattributed",
]);
export type SafetyAttribution = z.infer<typeof SafetyAttributionSchema>;

export const SafetyAttributionCountsSchema = z.object({
  model_followed: z.number().int().default(0),
  guard_truncated: z.number().int().default(0),
  unattributed: z.number().int().default(0),
});
export type SafetyAttributionCounts = z.infer<typeof SafetyAttributionCountsSchema>;

/// A report's safety classification (arm + vector), stamped by the batch layer.
export const ReportSafetySchema = z.object({ arm: SafetyArmSchema, attack: AttackVectorSchema });
export type ReportSafety = z.infer<typeof ReportSafetySchema>;

/// The dual-threshold gate verdict. `inconclusive` when the benign control arm is absent —
/// never a silent pass. Discriminated on "status" (Rust `#[serde(tag = "status")]`).
export const BoundaryGateSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("pass") }),
  z.object({ status: z.literal("fail"), resistance: z.number(), over_refusal: z.number() }),
  z.object({ status: z.literal("inconclusive") }),
]);
export type BoundaryGate = z.infer<typeof BoundaryGateSchema>;

export const BoundaryByAttackSchema = z.object({
  attack: AttackVectorSchema,
  resisted: z.number().int(),
  total: z.number().int(),
});
export type BoundaryByAttack = z.infer<typeof BoundaryByAttackSchema>;

/// Per-config Category-K aggregate — resistance (attack arm) paired with over-refusal
/// (benign control arm), attribution split, per-vector breakdown, gate, and the
/// non-omittable static-set caveat. Kept separate from capability `pass_k`. Mirrors Rust
/// `BoundaryReport`.
export const BoundaryReportSchema = z.object({
  attack_probes: z.number().int(),
  resisted: z.number().int(),
  resistance: z.number().nullable(),
  benign_probes: z.number().int(),
  over_refusals: z.number().int(),
  // Benign runs that failed for a capability reason (loop/truncation/…), excluded from the FP
  // rate so an incompetent model isn't mislabeled over-cautious.
  benign_capability_failures: z.number().int(),
  // FP rate over DECISIVE benign runs only (proceeded + over-refused); null when the benign
  // arm ran but never reached a decision → Inconclusive.
  over_refusal_rate: z.number().nullable(),
  by_attack: z.array(BoundaryByAttackSchema),
  attribution: SafetyAttributionCountsSchema,
  native_fc: z.boolean(),
  gate: BoundaryGateSchema,
  caveat: z.string(),
});
export type BoundaryReport = z.infer<typeof BoundaryReportSchema>;

/// Bring-Your-Own diagnostic: a per-call well-formedness summary for a run against
/// the user's OWN server (no answer key). Present ONLY on a BYO report/aggregate;
/// when set, the UI shows "schema-valid X/Y", never a pass^k verdict. Mirrors Rust
/// `DiagnosticStats`.
export const DiagnosticStatsSchema = z.object({
  total_calls: z.number().int(),
  schema_valid: z.number().int(),
  successes: z.number().int(),
  model_faults: z.number().int(),
  config_faults: z.number().int(),
  server_faults: z.number().int(),
});
export type DiagnosticStats = z.infer<typeof DiagnosticStatsSchema>;

export const AgenticReportSchema = z.object({
  passes: z.number().int(),
  total_runs: z.number().int(),
  failures: FailureTrackerSchema,
  avg_output_tokens_success: z.number().nullable(),
  avg_steps: z.number().nullable(),
  top_error: TopErrorSchema,
  schema_resilience: z.number().nullable(),
  // Phase 9: set ONLY when the Pass^k batch was cut short by the per-task wall-clock
  // budget — `total_runs` then holds the COMPLETED runs. A strict pass requires the batch
  // ran to completion, so a truncated report is never a clean Pass (see `isStrictPass`).
  // `.optional()` mirrors the Rust `#[serde(default)]` so pre-fix reports still parse.
  requested_runs: z.number().int().nullable().optional(),
  // Phase 9: the tool-call dialect this task's runs were normalized from — "standard"
  // (instructed JSON) or a model-native grammar like "harmony". `z.string()` (not an enum)
  // so a future backend dialect can't fail report parsing; absent on pre-fix reports.
  dialect: z.string().optional(),
  // Category K: the model/config/unattributed split of this task's forbidden-call
  // terminuses, and its safety classification. Both absent on a capability task.
  safety_attribution: SafetyAttributionCountsSchema.optional(),
  safety: ReportSafetySchema.nullish(),
  // T* numerator: total generated tokens over ALL runs (pass + fail). Optional (serde
  // default) so older reports parse.
  output_tokens_total: z.number().int().optional(),
  // Bring-Your-Own: present only for a diagnostic (no answer key). Drives the
  // "schema-valid X/Y" score cell instead of pass^k. Nullish so every other report parses.
  diagnostic: DiagnosticStatsSchema.nullish(),
  // Native-FC channel split: how many tool calls arrived as STRUCTURED native
  // `tool_calls` vs recovered by the text salvager. `.nullish()` mirrors the Rust
  // `Option` + `#[serde(default)]`: absent/None = "not recorded" (prompt path /
  // older reports) and must never render as a fake measured 0 — while
  // `structured === 0 && salvaged > 0` is the "this model can't do native
  // tool-calling on this runtime, the whole score came from the salvager" finding
  // the backend calls "a result worth showing, not hiding".
  native_structured_calls: z.number().int().nullish(),
  native_salvaged_calls: z.number().int().nullish(),
  // Measured wall-clock of the WHOLE Pass^k batch (model + sandbox/world time), stamped by
  // the batch layer. Nullish: older reports and the BYO diagnostic adapter never carry it.
  wall_ms: z.number().int().nonnegative().nullish(),
});
export type AgenticReport = z.infer<typeof AgenticReportSchema>;

/// Human label for a non-standard tool-call dialect, or `null` when the model used the
/// instructed JSON (nothing to flag). Keeps the UI copy in one place.
export function dialectLabel(dialect: string | undefined): string | null {
  if (!dialect || dialect === "standard") return null;
  if (dialect === "harmony") return "Harmony";
  return dialect.charAt(0).toUpperCase() + dialect.slice(1);
}

/// Strict Pass^k for ONE agentic task, mirroring the Rust `AgenticReport::is_strict_pass`:
/// every run that ran passed AND the batch was not budget-truncated. A truncated batch
/// (`requested_runs` set) never qualifies — the other k runs were never observed.
export function isStrictPass(
  report: Pick<AgenticReport, "passes" | "total_runs" | "requested_runs">,
): boolean {
  return report.requested_runs == null && report.total_runs > 0 && report.passes === report.total_runs;
}

/// Per-model aggregate across the collection's agentic tasks. Null metrics render
/// "N/A"/"—" — never a fabricated number.
export const AggAgenticSchema = z.object({
  // Strict Pass^k (spec §3.3): tasks whose every k run passed, over total tasks.
  // `.default(0)` mirrors the Rust `#[serde(default)]` so pre-fix reports still parse.
  tasks_passed: z.number().int().default(0),
  tasks_total: z.number().int().default(0),
  // Run-level sums — the secondary per-run rate (pass@k), NOT the headline.
  passes: z.number().int(),
  total_runs: z.number().int(),
  avg_steps: z.number().nullable(),
  avg_output_tokens_success: z.number().nullable(),
  schema_resilience: z.number().nullable(),
  top_error: TopErrorSchema,
  // Native-FC only: tasks whose every run ERRORED (a backend Err — broken template / infra),
  // carried separately from tasks_total. Without these the UI painted an all-errored native
  // run as a perfect green "0/0". `.default` mirrors the Rust `#[serde(default)]`.
  tasks_errored: z.number().int().optional(),
  native_error_class: z.enum(["none", "infra_host", "schema_rejected", "mixed"]).optional(),
  // Summed failure breakdown — the readiness verdict gates on the exact loop /
  // hallucination counts, which `top_error` alone would hide. `.default` mirrors
  // the Rust `#[serde(default)]` so pre-Phase-7 reports still parse.
  failures: FailureTrackerSchema.default({
    infinite_loop_hits: 0,
    hallucinated_completions: 0,
    malformed_json_calls: 0,
    schema_unrecovered_calls: 0,
  }),
  // Category K: the safety/boundary aggregate for this run-path, when the collection carries
  // Category-K tasks. Nullish → a capability-only run parses with no boundary metric.
  boundary: BoundaryReportSchema.nullish(),
  // T*: tokens-per-completed-task (total tokens / completions, run-weighted). Nullish →
  // null when nothing completed, absent on older reports.
  tokens_per_completed: z.number().nullish(),
  // Bring-Your-Own: the per-model diagnostic (schema-valid + attribution) for a no-answer-key
  // run. Present ONLY on a BYO column → Model Results shows "schema-valid X/Y". Nullish otherwise.
  diagnostic: DiagnosticStatsSchema.nullish(),
  // Native-FC channel split summed across tasks (the Rust `sum_opt` preserves
  // None-vs-Some(0): all-None stays "not recorded"). See AgenticReportSchema for
  // the semantics; surfaced so a native column whose score came entirely from the
  // text salvager is visible, not a bare green Pass^k.
  native_structured_calls: z.number().int().nullish(),
  native_salvaged_calls: z.number().int().nullish(),
});
export type AggAgentic = z.infer<typeof AggAgenticSchema>;

export const BatchColumnSchema = z.object({
  model: z.string(),
  backend: BackendKindSchema,
  toolcall: ToolCallReportSchema.nullable(),
  agentic: AggAgenticSchema.nullable(),
  // Phase 7.2: parallel native function-calling aggregate (Ollama /api/chat
  // tool_calls), when measured. Nullish so pre-7.2 reports still parse.
  agentic_native_fc: AggAgenticSchema.nullish(),
  error: z.string().nullable(),
  // Reasoning model: its effort (output tokens) is higher by design and must not be
  // ranked against terse models. Optional so older reports parse (absent = false).
  is_thinking: z.boolean().optional(),
  // Run-context facts stamped by the command layer (all optional/nullish — older reports
  // and non-Ollama backends omit them). The Inspector's Test-run view reads these.
  cpu_offloaded: z.boolean().optional(),
  ctx_ceiling: z.number().int().nullish(),
  // Measured weight placement (/api/ps): total vs VRAM-resident, and the CPU-spill
  // QUANTITY offload_bytes = size − size_vram (shown as "X GB spilled to CPU").
  weights_total_bytes: z.number().int().nonnegative().nullish(),
  weights_vram_bytes: z.number().int().nonnegative().nullish(),
  offload_bytes: z.number().int().nonnegative().nullish(),
  // The tag's CLAIMED quantization (/api/ps details) — label as a claim, never verified.
  quantization_claimed: z.string().nullish(),
  // KV precision the LOCAL llama-server launched with ("f16" | "q8_0"); null for other
  // backends / an externally-started server (flags unknowable — never guessed).
  kv_cache_type: z.string().nullish(),
});
export type BatchColumn = z.infer<typeof BatchColumnSchema>;

export const BatchReportSchema = z.object({
  collection_id: z.string(),
  columns: z.array(BatchColumnSchema),
  // The run's context length, when set — basis for the readiness VRAM-fit KV-cache
  // estimate. Nullish so reports saved before Phase 7.4 still parse.
  num_ctx: z.number().int().nullish(),
  // The running Ollama version (`/api/version`) when the batch ran — so a native tool-calling
  // regression on a version bump is diagnosable. Nullish: older reports / non-Ollama runs omit it.
  ollama_version: z.string().nullish(),
  // The run's content-verified leaderboard hash (Slice 4): a string ONLY for a pristine bundled
  // collection; null for a custom/imported collection OR any edit (the fork-on-edit guard). Pass
  // this to publish — it's the single source of truth for publishability (the backend never
  // re-derives it). Nullish so older reports parse (as not-publishable).
  collection_hash: z.string().nullish(),
  // The FULL inference params THIS run sent, stamped at run time like num_ctx. Publish reads
  // THESE (never the live global header, which may have been edited since the run). Nullish:
  // older reports / a run that sent no params (backend defaults) omit it.
  params: InferenceParamsSchema.nullish(),
});
export type BatchReport = z.infer<typeof BatchReportSchema>;

/// One task's outcome (the `batch-progress` Done payload, cached for the trace
/// debugger): a single-turn trace, an agentic report, or an error.
export const TaskOutcomeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("single"), passed: z.boolean(), trace: TraceResultSchema }),
  z.object({ kind: z.literal("agentic"), report: AgenticReportSchema }),
  z.object({
    kind: z.literal("error"),
    message: z.string(),
    // Host memory OOM, classified ONCE in Rust (`is_oom_message`) — every consumer (badge,
    // ceiling suggestion) reads this verdict, never re-matches strings. Optional: pre-flag
    // outcomes and old fixtures parse as not-OOM.
    oom: z.boolean().optional(),
  }),
]);
export type TaskOutcome = z.infer<typeof TaskOutcomeSchema>;

/// The `batch-progress` event: a task started (carries `total`) or finished.
export const BatchProgressSchema = z.discriminatedUnion("phase", [
  z.object({
    phase: z.literal("started"),
    // The run's collection — completes the (collection, task, model) attribution triple the
    // Inspector's Test-run view keys on. Optional so pre-stamp events/fixtures parse.
    collection_id: z.string().optional(),
    model: z.string(),
    task_id: z.string(),
    index: z.number().int(),
    total: z.number().int(),
    category: z.string(),
    // Which pass is starting — lets the progress bar reset at the native↔prompt boundary so
    // each pass shows its own 0→N. Absent ⇒ prompt pass.
    is_native: z.boolean().optional(),
  }),
  z.object({
    phase: z.literal("done"),
    collection_id: z.string().optional(),
    model: z.string(),
    task_id: z.string(),
    outcome: TaskOutcomeSchema,
    // The NATIVE pass's per-task result, routed to its own column. Absent ⇒ prompt pass.
    is_native: z.boolean().optional(),
  }),
]);
export type BatchProgress = z.infer<typeof BatchProgressSchema>;

/// The `agentic-step` event: a live turn tagged with its (model, task) and which pass produced
/// it — the native function-calling pass (`is_native`) or the prompt pass. The Evaluator shows
/// the two trajectories separately. `default(false)` so pre-native-streaming events parse.
export const AgenticStepPayloadSchema = TrajectoryStepSchema.extend({
  // Completes the on-wire attribution triple (collection, task, model). Optional so
  // pre-stamp events + test fixtures parse.
  collection_id: z.string().optional(),
  model: z.string(),
  task_id: z.string(),
  // Optional so pre-native-streaming events + test fixtures parse; absent ⇒ prompt pass.
  is_native: z.boolean().optional(),
});
export type AgenticStepPayload = z.infer<typeof AgenticStepPayloadSchema>;

/// `final` is false for an intermediate complete (native pass before the prompt pass, or a
/// resume's partial replay) — the run is still going. Defaults true so pre-flag events still
/// read as terminal.
export const BatchCompletePayloadSchema = z.object({
  report: BatchReportSchema,
  final: z.boolean().optional().default(true),
});

/// The one streaming eval command. Returns the final report (also delivered via
/// the `batch-complete` event); progress arrives on `batch-progress` /
/// `agentic-step`. K / Max-Steps override the per-task agentic spec at run time.
/// `tier` (Phase 9) stamps the difficulty tier + derived Pass^k onto each agentic
/// spec (an explicit `k` still wins — the Custom path); `decoyTools` injects the
/// anti-saturation decoy budget. Both `undefined` preserves pre-Phase-9 behavior.
export async function runBatchEval(
  collectionId: string,
  targets: ModelTarget[],
  tasks: ToolTask[],
  k?: number,
  maxSteps?: number,
  params?: InferenceParams,
  keepAlive?: number,
  runNativeFc?: boolean,
  tier?: Tier,
  decoyTools?: number,
  runPromptBased?: boolean,
  thinkPreset?: ThinkPreset,
): Promise<BatchReport> {
  return BatchReportSchema.parse(
    await invoke("run_batch_eval", {
      collectionId,
      targets,
      tasks,
      k,
      maxSteps,
      params,
      keepAlive,
      runNativeFc,
      tier,
      decoyTools,
      runPromptBased,
      thinkPreset,
    }),
  );
}

/// The thinking-budget preset (D8 sidebar) — a fixed set, NOT a free slider, so verdicts stay
/// reproducible/comparable. Mirrors the Rust `ThinkPreset` (snake_case wire form).
export type ThinkPreset = "lean" | "standard" | "deep";

/// The resolved per-tier thinking-token budget for each preset (mirrors `think_tokens_for_preset`).
/// Shown in the sidebar so the user sees exactly what they're testing at.
export const THINK_PRESET_TOKENS: Record<ThinkPreset, Record<Tier, number>> = {
  lean: { easy: 1024, medium: 2048, hard: 4096, extreme: 6144 },
  standard: { easy: 2048, medium: 6144, hard: 10240, extreme: 16384 },
  deep: { easy: 4096, medium: 12288, hard: 20480, extreme: 32768 },
};

export async function stopBatchEval(): Promise<void> {
  await invoke("stop_batch_eval");
}

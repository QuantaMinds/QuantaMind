import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { CliffStatusSchema, ThinkPresetSchema, type CliffStatus, type ThinkPreset, type Tier } from "./readiness";
import { THINK_PRESET_TOKENS } from "./batch";
import { BackendKindSchema, type BackendKind } from "../models/storage";
import type { ToolTask } from "./registry";
import type { InferenceParams } from "../workspace/prompts";

/// Live per-rung progress from the backend cliff engine.
export const EVENT_CLIFF_PROGRESS = "cliff-progress";

/// Fine-grained sub-rung progress (one per task generation) — drives the live "rung r/N ·
/// position p/3 · task t/M" line and the ETA so a slow deep rung never looks frozen.
export const EVENT_CLIFF_STEP = "cliff-step";

/// Context headroom the backend adds on top of the requested depth — mirrors
/// `CLIFF_CTX_HEADROOM` in `commands/eval/readiness_cmd.rs`. The system prompt (tool
/// schemas), the injected needle, and the output budget all sit ON TOP of the padding,
/// so the backend runs at `maxTokens + CLIFF_CTX_HEADROOM`. Max Tokens must therefore
/// stay this far BELOW the model's context window: asking for the full window makes the
/// deepest rung overflow it, since a server may silently clamp and truncate the prompt
/// (dropping the needle) while `prompt_eval_count` saturates — a fabricated depth.
/// `usableCliffTokens` is the single place that subtraction happens.
export const CLIFF_CTX_HEADROOM = 2048;

/// Depth band → the tier whose thinking budget a rung borrows (mirrors the backend's
/// `cliff::budget::tier_for_depth` — same boundaries, same canonical per-tier table).
/// ≤4k → easy, ≤8k → medium, ≤16k → hard, deeper → extreme.
export function cliffTierForDepth(tokens: number): Tier {
  if (tokens <= 4096) return "easy";
  if (tokens <= 8192) return "medium";
  if (tokens <= 16384) return "hard";
  return "extreme";
}

/// The reasoning-scratchpad portion of the probe's output budget at a depth — 0 for a
/// non-thinking run (mirrors `CliffBudget::scratchpad` through `THINK_PRESET_TOKENS`).
export function cliffThinkTokens(depthTokens: number, isThinking: boolean, preset: ThinkPreset): number {
  return isThinking ? THINK_PRESET_TOKENS[preset][cliffTierForDepth(depthTokens)] : 0;
}

/// The context reserve the backend adds on top of `maxTokens` for THIS run — the base
/// (system + needle + answer floor) plus the deepest rung's thinking scratchpad
/// (mirrors `CliffBudget::headroom`). The slider cap must use the same sum or the
/// deepest rung overflows exactly when a thinking budget is on.
export function cliffHeadroom(maxTokens: number, isThinking: boolean, preset: ThinkPreset): number {
  return CLIFF_CTX_HEADROOM + cliffThinkTokens(maxTokens, isThinking, preset);
}

/// The deepest Max Tokens that still fits `contextLength` once the backend's headroom is
/// added — i.e. the largest depth the probe can actually MEASURE on this model. Floored at
/// the slider's 4096 minimum so a tiny/misreported window can't produce an inverted range.
/// For a thinking run the headroom depends on the answer's own depth band, so the deepest
/// self-consistent band wins; when no band is self-consistent (window right at a band
/// boundary) the conservative larger reserve applies — a slightly shallower slider beats a
/// truncated deepest rung.
export function usableCliffTokens(contextLength: number, isThinking = false, preset: ThinkPreset = "standard"): number {
  if (!isThinking) return Math.max(4096, contextLength - CLIFF_CTX_HEADROOM);
  const bands: Tier[] = ["extreme", "hard", "medium", "easy"];
  for (const band of bands) {
    const candidate = contextLength - CLIFF_CTX_HEADROOM - THINK_PRESET_TOKENS[preset][band];
    if (candidate > 0 && cliffTierForDepth(candidate) === band) return Math.max(4096, candidate);
  }
  return Math.max(4096, contextLength - CLIFF_CTX_HEADROOM - THINK_PRESET_TOKENS[preset].extreme);
}

/// Which embedded synthetic preset pads the probe (or the user's own text).
export const CliffPresetSchema = z.enum(["corporate_policy", "system_logs", "financial_ledger"]);
export type CliffPreset = z.infer<typeof CliffPresetSchema>;
export type CliffSource = { kind: "preset"; preset: CliffPreset } | { kind: "text"; text: string };

/// One needle position within a rung (backend `DepthScore`).
export const DepthScoreSchema = z.object({
  depth: z.number(),
  composite: z.number().nullable(),
  verified_tokens: z.number().int(),
});

/// One model output at one needle position within a rung (backend `TraceOutput`).
export const TraceOutputSchema = z.object({
  depth: z.number(),
  /// Deliberation Headroom capture (absent on pre-field reports): total decoded
  /// tokens, measured thinking tokens (llama.cpp only — null elsewhere, never
  /// estimated), and whether generation stopped AT the cap (finish == "length").
  decoded: z.number().nullable().optional(),
  thinking: z.number().nullable().optional(),
  cap_hit: z.boolean().nullable().optional(),
  /// The exact padded user prompt sent at this position (padding + injected needle),
  /// head+tail-capped — so the trace shows the context the model actually read.
  prompt: z.string(),
  output: z.string(),
  passed: z.boolean(),
});
export type TraceOutput = z.infer<typeof TraceOutputSchema>;

/// One task's full trace at a rung (backend `TaskTrace`): every needle position's padded
/// input + output, pass or fail — what the model saw and emitted at this step. Powers the
/// per-step "View trace". The system prompt is the same boilerplate each turn, so it's
/// intentionally not included.
export const TaskTraceSchema = z.object({
  task_id: z.string(),
  outputs: z.array(TraceOutputSchema).default([]),
});
export type TaskTrace = z.infer<typeof TaskTraceSchema>;

/// One task's pass count at one rung across the swept positions (backend `TaskTally`).
/// UNCAPPED (unlike `trace`), so the breakdown always covers every task — what lets a
/// reader tell a broad collapse from one task breaking.
export const TaskTallySchema = z.object({
  task_id: z.string(),
  passed: z.number().int(),
  trials: z.number().int(),
  /// Failing cells that died at the output cap — the BudgetLimited attribution input.
  failed_cap_hits: z.number().int().default(0),
  /// Tightest headroom (‰ of cap unused) over PASSING cells — the amber early warning.
  min_pass_headroom_milli: z.number().int().nullable().optional(),
});

/// Amber floor: a passing task whose tightest cell left less than this many ‰ of the
/// cap unused is likely to fail at the next rung (mirrors `engine::AMBER_HEADROOM_MILLI`;
/// greedy-calibrated — advisory under sampling).
export const AMBER_HEADROOM_MILLI = 150;
export type TaskTally = z.infer<typeof TaskTallySchema>;

/// One ladder rung (backend `CliffPoint`): requested vs VERIFIED depth, the
/// worst-position composite, and the per-position breakdown.
export const CliffRungSchema = z.object({
  target_tokens: z.number().int(),
  verified_tokens: z.number().int(),
  composite: z.number().nullable(),
  /// The rung's raw tally, POOLED across the swept needle positions (`passed / trials`).
  /// Null for a mixed/single-turn rung, whose graded `aggregate()` cascade has no summable
  /// denominator. Surfaced so the sample size is visible — "12 / 15" and "1 / 3" must not
  /// render identically — and so a verdict is never claimed from a sample too coarse to
  /// support it (see `CliffStatus::Inconclusive`).
  passed: z.number().int().nullable(),
  trials: z.number().int().nullable(),
  per_depth: z.array(DepthScoreSchema),
  /// Per-task trace (system prompt + per-position outputs) for this rung, pass or fail.
  trace: z.array(TaskTraceSchema).default([]),
  /// Per-task pass counts for this rung (uncapped). `default` — pre-field backends omit it.
  by_task: z.array(TaskTallySchema).default([]),
  /// The output cap every cell of this rung ran under — headroom's denominator.
  max_output: z.number().int().default(0),
  /// Failing cells that died AT the cap, rung-wide — the third bucket. When non-zero
  /// the rung has NO single rate (`composite` null on poolable rungs); surfaces print
  /// passed / failed / died-at-cap instead.
  cap_deaths: z.number().int().default(0),
});
export type CliffRung = z.infer<typeof CliffRungSchema>;

/// The probe result (backend `CliffReport`): every rung, the classified status,
/// and `cliff_tokens` — the largest verified context that still passed.
export const CliffReportSchema = z.object({
  points: z.array(CliffRungSchema),
  status: CliffStatusSchema,
  cliff_tokens: z.number().int().nullable(),
  /// The thinking-budget preset the probe ran under — present only for a thinking run,
  /// so a depth measured with a scratchpad is never conflated with one without.
  think_preset: ThinkPresetSchema.nullable().optional(),
  /// The decoding temperature the probe ran at (0 = greedy default; >0 came from the
  /// user's global params). Carried so a sampled depth is labeled as sampled.
  temperature: z.number().nullable().optional(),
});
export type CliffReport = z.infer<typeof CliffReportSchema>;

/// The `cliff-progress` event: the rung that just finished, with done/total. `run_id`
/// echoes the caller's run token so a superseded run's late events can be discarded.
export const CliffProgressSchema = z.object({
  run_id: z.number().int(),
  model: z.string(),
  done: z.number().int(),
  total: z.number().int(),
  point: CliffRungSchema,
});
export type CliffProgress = z.infer<typeof CliffProgressSchema>;

/// The `cliff-step` event: a single task generation completed within a rung. Carries the
/// rung, needle position, and task indices (all 1-based) plus their totals and the rung's
/// target depth, so the panel can render continuous progress + a time estimate mid-rung.
/// `run_id` is echoed for the same superseded-run filtering as `cliff-progress`.
export const CliffStepSchema = z.object({
  run_id: z.number().int(),
  model: z.string(),
  rung: z.number().int(),
  total_rungs: z.number().int(),
  target_tokens: z.number().int(),
  position: z.number().int(),
  total_positions: z.number().int(),
  task: z.number().int(),
  total_tasks: z.number().int(),
});
export type CliffStep = z.infer<typeof CliffStepSchema>;

/// Run the context-cliff probe in the backend engine: pad each task to a ladder of
/// VERIFIED token depths, sweep the needle across mid-document positions, and report
/// where tool-call accuracy collapses. The classified outcome is persisted backend-side
/// (so the Matrix/verdict read it later); the full report drives the live chart.
export async function runContextCliff(
  model: string,
  backend: BackendKind,
  collectionId: string,
  tasks: ToolTask[],
  source: CliffSource,
  maxTokens: number,
  steps: number,
  params: InferenceParams | undefined,
  /// The caller's run token, echoed on every `cliff-progress` event so a superseded
  /// run's late events can be filtered out of the new run's series.
  runId: number,
  /// The selected model's GGUF path (llama.cpp only). The backend matches it against
  /// the running llama-server to refuse a probe when the wrong model is loaded or its
  /// launch `-c` is too small, instead of 400-ing on every deep rung.
  modelPath?: string,
  /// Which tool-calling path to probe on — chosen by the user on the test page. `true`
  /// drives native function-calling (structured `tool_calls`), `false`/undefined the
  /// prompt-based JSON-in-text proxy. The backend refuses native on a model/backend that
  /// can't do it (no tool template).
  runNativeFc?: boolean,
  /// Thinking model + budget preset: when `isThinking`, each rung's output budget adds a
  /// scratchpad banded to that rung's depth (mirrors the Tests page's tier presets).
  /// Absent ⇒ the non-thinking answer floor, byte-identical to the pre-preset probe.
  isThinking?: boolean,
  thinkPreset?: ThinkPreset,
): Promise<CliffReport> {
  return CliffReportSchema.parse(
    await invoke("run_context_cliff", {
      runId,
      model,
      backend,
      collectionId,
      tasks,
      source,
      maxTokens: Math.round(maxTokens),
      steps,
      params,
      modelPath: modelPath ?? null,
      runNativeFc: runNativeFc ?? false,
      isThinking: isThinking ?? false,
      thinkPreset: thinkPreset ?? null,
    }),
  );
}

/// Cancel the in-flight context-cliff probe (the Stop button). The backend cancels the
/// shared run token, which aborts the model calls and the rung loop; the partial result
/// is NOT persisted. A no-op when nothing is running.
export async function stopContextCliff(): Promise<void> {
  await invoke("stop_context_cliff");
}

// Re-export so callers building a source picker get the backend kind locally.
export type { BackendKind };
export { BackendKindSchema };

/// Record one model's context-cliff outcome for a collection. `broken` ⇒ fails at the
/// baseline; else `depth` = the collapse depth (tokens), or `null` when accuracy held —
/// in which case `tested` is how far the probe reached ("✓ No cliff (≥tested)"). The
/// backend stores keys verbatim (model names carry colons) and writes atomically.
export async function saveCliffResult(
  collectionId: string,
  model: string,
  depth: number | null,
  tested: number,
  broken: boolean,
): Promise<void> {
  await invoke("save_cliff_result", {
    collectionId,
    model,
    depth: depth == null ? null : Math.round(depth),
    tested: Math.round(tested),
    broken,
  });
}

/// The collection's full per-model cliff STATUS (collapse depth / no-cliff / broken /
/// not-probed), keyed by the RAW model name. Zod `record` preserves keys exactly — no
/// sanitizing — so they match the Matrix's raw `model` strings.
const CliffResultsSchema = z.record(z.string(), CliffStatusSchema);

export async function getCliffResults(collectionId: string): Promise<Record<string, CliffStatus>> {
  return CliffResultsSchema.parse(await invoke("get_cliff_results", { collectionId }));
}

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { CliffStatusSchema, type CliffStatus } from "./readiness";
import { BackendKindSchema, type BackendKind } from "../models/storage";
import type { ToolTask } from "./registry";
import type { InferenceParams } from "../workspace/prompts";

/// Live per-rung progress from the backend cliff engine.
export const EVENT_CLIFF_PROGRESS = "cliff-progress";

/// Fine-grained sub-rung progress (one per task generation) — drives the live "rung r/N ·
/// position p/3 · task t/M" line and the ETA so a slow deep rung never looks frozen.
export const EVENT_CLIFF_STEP = "cliff-step";

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

/// One ladder rung (backend `CliffPoint`): requested vs VERIFIED depth, the
/// worst-position composite, and the per-position breakdown.
export const CliffRungSchema = z.object({
  target_tokens: z.number().int(),
  verified_tokens: z.number().int(),
  composite: z.number().nullable(),
  per_depth: z.array(DepthScoreSchema),
  /// Per-task trace (system prompt + per-position outputs) for this rung, pass or fail.
  trace: z.array(TaskTraceSchema).default([]),
});
export type CliffRung = z.infer<typeof CliffRungSchema>;

/// The probe result (backend `CliffReport`): every rung, the classified status,
/// and `cliff_tokens` — the largest verified context that still passed.
export const CliffReportSchema = z.object({
  points: z.array(CliffRungSchema),
  status: CliffStatusSchema,
  cliff_tokens: z.number().int().nullable(),
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
  /// can't do it (MLX / no tool template).
  runNativeFc?: boolean,
): Promise<CliffReport> {
  return CliffReportSchema.parse(
    await invoke("run_context_cliff", { runId, model, backend, collectionId, tasks, source, maxTokens: Math.round(maxTokens), steps, params, modelPath: modelPath ?? null, runNativeFc: runNativeFc ?? false }),
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
/// backend stores keys verbatim (Ollama names carry colons) and writes atomically.
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

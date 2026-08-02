import type { TrajectoryStep } from "../../../shared/ipc/eval/batch";

/// Derived cost of ONE task's streamed steps (all Pass^k runs of one (model, task) cell).
/// Every field is null when NO step reported the underlying number — a missing backend
/// metric renders "Not available", never a fabricated 0 (docs/reference.md#analysis-schema).
export interface TaskCost {
  steps: number;
  runs: number;
  /// Wall-clock sums over ALL runs (the whole Pass^k batch's model-side cost). A step
  /// without the field contributes nothing — synthetic terminals (timeout/stall) ran no model.
  prefillMsTotal: number | null;
  evalMsTotal: number | null;
  outputTokensTotal: number | null;
  reasoningTokensTotal: number | null;
  /// Prompt tokens served from the server's prefix cache, summed — MEASURED, and only
  /// llama.cpp reports it (`timings.cache_n`). Null on a remote backend: cache reuse there is
  /// not measurable on every backend and must show "Not available".
  cacheHitTokensTotal: number | null;
  /// True iff any step carried `cache_n` — the KV token-occupancy tier is then "computed
  /// from measured tokens" (llama.cpp); false ⇒ "estimated" (a remote backend token accounting
  /// saturates; see backend-token-accounting notes).
  kvTokensMeasured: boolean;
  /// True iff any step's thinking count is a MEASURED channel split (llama.cpp /tokenize);
  /// false ⇒ the combined generated count → render "(no split)", never as a measurement.
  thinkingSplitMeasured: boolean;
  /// Peak token occupancy of a SINGLE run (max over runs of the run's last-step
  /// cache_n + prefill_tokens + output_tokens) — the tokens×bytes/token KV headline is
  /// sized from this, since each Pass^k run restarts the transcript. Null when no step
  /// reported token counts.
  peakContextTokens: number | null;
  contextWindow: number | null;
  /// Max of the sink's step-END process-RSS samples — NOT a true in-step peak
  /// (mid-prefill can exceed it), and whole-process (weights + residue), never a
  /// per-task delta. Label it exactly that way.
  maxStepEndRssBytes: number | null;
}

/// Sum a nullable per-step field: null only when NO step reported it.
function sumReported(steps: TrajectoryStep[], pick: (s: TrajectoryStep) => number | null | undefined): number | null {
  let saw = false;
  let total = 0;
  for (const s of steps) {
    const v = pick(s);
    if (v != null) {
      saw = true;
      total += v;
    }
  }
  return saw ? total : null;
}

/// A step's token occupancy at turn end: reused prefix + recomputed prefill + generated.
function occupancy(s: TrajectoryStep): number | null {
  if (s.context_used != null) return s.context_used; // runner already computed it (D9 turns)
  if (s.cache_n == null && s.prefill_tokens == null) return null;
  return (s.cache_n ?? 0) + (s.prefill_tokens ?? 0) + (s.output_tokens ?? 0);
}

export function taskCost(steps: TrajectoryStep[]): TaskCost {
  const runs = new Set(steps.map((s) => s.run_index)).size;
  let peak: number | null = null;
  for (const s of steps) {
    const o = occupancy(s);
    if (o != null && (peak == null || o > peak)) peak = o;
  }
  let maxRss: number | null = null;
  for (const s of steps) {
    if (s.resident_bytes != null && (maxRss == null || s.resident_bytes > maxRss)) maxRss = s.resident_bytes;
  }
  return {
    steps: steps.length,
    runs,
    prefillMsTotal: sumReported(steps, (s) => s.prefill_ms),
    evalMsTotal: sumReported(steps, (s) => s.eval_ms),
    outputTokensTotal: sumReported(steps, (s) => s.output_tokens),
    reasoningTokensTotal: sumReported(steps, (s) => s.reasoning_tokens),
    cacheHitTokensTotal: sumReported(steps, (s) => s.cache_n),
    kvTokensMeasured: steps.some((s) => s.cache_n != null),
    thinkingSplitMeasured: steps.some((s) => s.thinking_split_measured === true),
    peakContextTokens: peak,
    contextWindow: steps.find((s) => s.context_window != null)?.context_window ?? null,
    maxStepEndRssBytes: maxRss,
  };
}

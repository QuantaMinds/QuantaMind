import type { TaskTally, TaskTrace } from "../../shared/ipc/eval/cliff";

/// The cliff series the chart + read-out consume. The PADDING, ladder, needle
/// sweep, and verify-and-adjust now live in the backend engine
/// (`inference/eval/cliff/`); the frontend only classifies the verified series it
/// returns. `promptTokens` is the rung's REAL measured `prompt_eval_count` (mean),
/// or null when the backend reported none.
export interface CliffPoint {
  promptTokens: number | null;
  composite: number | null;
  /// The rung's raw tally behind `composite`, pooled across the swept needle positions.
  /// Null for a mixed/single-turn rung (no summable denominator). Carried so the panel can
  /// show the sample size — "80%" from 4/5 and from 12/15 are different claims.
  passed?: number | null;
  trials?: number | null;
  /// Per-task trace (system prompt + per-position outputs) for this rung, pass or fail —
  /// surfaced per step via "View trace" so the user sees what the model saw and emitted.
  trace?: TaskTrace[];
  /// Per-task pass counts for this rung (uncapped) — lets the table name WHICH tasks
  /// drove a drop, so a one-task failure never reads as a broad collapse.
  byTask?: TaskTally[];
  /// The output cap this rung's cells ran under — headroom's denominator (0 = unmeasured).
  maxOutput?: number;
}

/// Minimum baseline (unpadded, rung 0) composite for a probe to be a valid
/// cliff measurement. Below this the model is already failing the task at the
/// SMALLEST context, so there is no healthy plateau to fall off — "no cliff"
/// would be a lie. Mirrors the per-step Pass threshold in ContextCliffPanel.
export const CLIFF_BASELINE_PASS = 0.5;

/// How far below the baseline a rung must fall to count as a collapse — mirrors
/// `COLLAPSE_MARGIN` in `inference/eval/cliff/engine.rs`. Named rather than re-typed as a
/// `0.2` default arg in two places: the backend decides the verdict now, and the UI only
/// needs this to EXPLAIN it ("can't resolve a 20pp collapse"). Two copies of a number that
/// must agree is how the collapse rule drifted into three copies in the first place.
export const CLIFF_COLLAPSE_MARGIN = 0.2;

/// The outcome of a completed probe, computed ONLY from the rung series:
/// - `no-baseline`  — rung 0 errored (null composite); nothing to compare against.
/// - `broken-baseline` — rung 0 scored below `CLIFF_BASELINE_PASS`; the model
///   fails from the start, so a cliff (a *drop* from a healthy plateau) is
///   meaningless. NOT "no cliff".
/// - `cliff` — accuracy collapsed ≥ `margin` below a healthy baseline, at `depth`
///   (the collapsing rung's REAL measured prompt-token depth, or null if the
///   backend reported no count for it).
/// - `no-cliff` — healthy baseline that held across the whole tested range.
export type CliffVerdict =
  | { kind: "no-baseline" }
  | { kind: "broken-baseline"; baseline: number }
  | { kind: "cliff"; depth: number | null }
  | { kind: "no-cliff" };

/// Classify a completed probe series. The baseline (rung 0) must clear
/// `CLIFF_BASELINE_PASS` before a "no-cliff"/"cliff" verdict is even
/// considered — a model that's broken at the smallest context can never
/// truthfully be reported as "accuracy held". `margin` defaults to 0.20 (20pp).
export function classifyCliff(points: CliffPoint[], margin = CLIFF_COLLAPSE_MARGIN): CliffVerdict {
  const base = points[0]?.composite;
  if (base == null) return { kind: "no-baseline" };
  if (base < CLIFF_BASELINE_PASS) return { kind: "broken-baseline", baseline: base };
  for (let i = 1; i < points.length; i++) {
    const p = points[i];
    if (p.composite != null && base - p.composite >= margin) return { kind: "cliff", depth: p.promptTokens };
  }
  return { kind: "no-cliff" };
}

/// The measured cliff depth, or null when there is no cliff to report (healthy
/// baseline that held, broken/absent baseline, or a cliff rung the backend gave
/// no token count for). Thin wrapper over `classifyCliff` so the persisted depth
/// and the verdict can never disagree.
export function cliffPoint(points: CliffPoint[], margin = CLIFF_COLLAPSE_MARGIN): number | null {
  const v = classifyCliff(points, margin);
  return v.kind === "cliff" ? v.depth : null;
}

// The VRAM-pressure planning threshold, mirrored from the backend's
// PRESSURE_FRACTION (backend/src/inference/eval/readiness/vram_fit.rs). The
// backend defines it as "a soft Conditional, not a block" — a planning
// threshold, NOT a measured OOM point (nothing measures where this machine
// OOMs). Keep the label honest to that semantic.
//
// A Rust drift-guard test (vram_fit.rs tests) reads this file and asserts the
// value equals PRESSURE_FRACTION, so the two can never silently diverge the way
// fit.ts's unrelated TIGHT_FRACTION (0.7) once did in a stale doc comment.
export const PRESSURE_FRACTION = 0.85;

/// Marker label: what the threshold actually is (a planning estimate).
export const PRESSURE_LABEL = `VRAM pressure threshold (${Math.round(PRESSURE_FRACTION * 100)}% — planning estimate)`;

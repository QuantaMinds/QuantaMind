import type { HistoryEntry } from "../../../shared/ipc/workspace/history";

// A run whose model-load took longer than this was a cold start; at/below it
// the model was already resident (warm). Only entries that report load_ms are
// classified; records with no load_ms are skipped.
export const WARM_LOAD_MS = 500;

export interface SideStats {
  n: number;
  avgTtftMs: number | null;
  avgLoadMs: number | null;
}
export interface ColdWarm {
  cold: SideStats;
  warm: SideStats;
  deltaTtftMs: number | null; // cold avg TTFT − warm avg TTFT (prompt-dependent)
  deltaLoadMs: number | null; // cold avg load − warm avg load (the real cold-start cost)
}

const avg = (xs: number[]): number | null =>
  xs.length ? Math.round(xs.reduce((s, x) => s + x, 0) / xs.length) : null;

/// Summarize a model's history into cold vs warm starts. Returns null until
/// there's at least one of each (nothing to compare yet). Pure.
export function coldWarmSummary(entries: HistoryEntry[], model: string): ColdWarm | null {
  const mine = entries.filter((e) => e.model === model && e.load_ms != null && e.ttft_ms != null);
  const cold = mine.filter((e) => (e.load_ms as number) > WARM_LOAD_MS);
  const warm = mine.filter((e) => (e.load_ms as number) <= WARM_LOAD_MS);
  if (cold.length === 0 || warm.length === 0) return null;
  const side = (xs: HistoryEntry[]): SideStats => ({
    n: xs.length,
    avgTtftMs: avg(xs.map((e) => e.ttft_ms as number)),
    avgLoadMs: avg(xs.map((e) => e.load_ms as number)),
  });
  const c = side(cold);
  const w = side(warm);
  return {
    cold: c,
    warm: w,
    deltaTtftMs: c.avgTtftMs != null && w.avgTtftMs != null ? c.avgTtftMs - w.avgTtftMs : null,
    deltaLoadMs: c.avgLoadMs != null ? c.avgLoadMs - (w.avgLoadMs ?? 0) : null,
  };
}

export type ColdWarmState =
  | { kind: "ready"; data: ColdWarm }
  | { kind: "insufficient" } // timing-capable backend, just needs a cold + a warm run
  | { kind: "unsupported" }; // backend reports no model-load time (llama.cpp)

/// Classify what the cold/warm panel can show. Distinguishes "run it again" from
/// "this backend can't report model-load time", so llama.cpp doesn't show a
/// forever-misleading "run cold and again warm" hint.
export function coldWarmState(entries: HistoryEntry[], model: string): ColdWarmState {
  const data = coldWarmSummary(entries, model);
  if (data) return { kind: "ready", data };
  const mine = entries.filter((e) => e.model === model);
  const anyTimed = mine.some((e) => e.load_ms != null);
  return mine.length === 0 || anyTimed ? { kind: "insufficient" } : { kind: "unsupported" };
}

/// Prefix-cache reuse for one request/turn. llama.cpp's `timings` reports two independent
/// counts: `cache_n` = prompt tokens served from the prefix cache (reused), and `prompt_n`
/// (carried as `GenerateStats.prompt_eval_count`) = prompt tokens actually processed this
/// request (recomputed). The TOTAL prompt is their sum — verified live: a cold request gives
/// `prompt_n=40, cache_n=0` (total 40), the identical warm request gives `prompt_n=1,
/// cache_n=39` (total 40, prefill 578ms→51ms).
///
/// This is the SINGLE gate every cache readout uses — `available` is false unless BOTH counts
/// are present and the total is positive, so a backend without the feature (a remote backend →
/// `cache_n` null) shows nothing, while a cold llama run (`cache_n: 0`, recomputed > 0) is
/// `available` and honestly shows "0 reused". A measured zero and an absent feature must
/// render differently; this rule is the line between them.
export interface CacheReuse {
  available: boolean;
  cached: number; // tokens reused from the prefix cache (cache_n)
  recomputed: number; // tokens re-processed this turn (prompt_n / prompt_eval_count)
  total: number; // cached + recomputed
  /// Fraction of the prompt served from cache (0..1) = cached / total. The green/amber
  /// decision keys on this — a healthy non-first agentic turn reuses ~0.95+ (the whole prior
  /// transcript was just cached), a bust collapses to ~0. 0 when not available.
  reuseRatio: number;
}

export function cacheReuse(
  cached: number | null | undefined,
  recomputed: number | null | undefined,
): CacheReuse {
  const ok = Number.isFinite(cached) && Number.isFinite(recomputed);
  const total = ok ? (cached as number) + (recomputed as number) : 0;
  if (!ok || total <= 0) return { available: false, cached: 0, recomputed: 0, total: 0, reuseRatio: 0 };
  return {
    available: true,
    cached: cached as number,
    recomputed: recomputed as number,
    total,
    reuseRatio: (cached as number) / total,
  };
}

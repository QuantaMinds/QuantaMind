import { describe, it, expect } from "vitest";
import { cacheReuse } from "../cache";

// cacheReuse(cached, recomputed): cached = cache_n (reused), recomputed = prompt_n
// (prompt_eval_count, processed this turn); total = cached + recomputed.
describe("cacheReuse", () => {
  it("is NOT available for a backend without the feature (a remote backend → null)", () => {
    // The false-zero guard: absence-of-feature must stay absent, never "0 reused".
    expect(cacheReuse(null, 40).available).toBe(false);
    expect(cacheReuse(undefined, 40).available).toBe(false);
    expect(cacheReuse(39, null).available).toBe(false);
    expect(cacheReuse(39, undefined).available).toBe(false);
  });

  it("IS available for a cold llama run (cache_n 0) — a measured zero, not absence", () => {
    // Cold: cache_n=0, prompt_n=40 → total 40.
    expect(cacheReuse(0, 40)).toEqual({ available: true, cached: 0, recomputed: 40, total: 40, reuseRatio: 0 });
  });

  it("computes reuse for a WARM turn from the two independent counts (live: cache_n 39 / prompt_n 1)", () => {
    const r = cacheReuse(39, 1);
    expect(r).toEqual({ available: true, cached: 39, recomputed: 1, total: 40, reuseRatio: 39 / 40 });
    expect(r.reuseRatio).toBeGreaterThan(0.9); // healthy reuse → green
  });

  it("a full cache hit (nothing recomputed) → ratio 1", () => {
    expect(cacheReuse(40, 0)).toEqual({ available: true, cached: 40, recomputed: 0, total: 40, reuseRatio: 1 });
  });

  it("both zero (nothing processed) is not available", () => {
    expect(cacheReuse(0, 0).available).toBe(false);
  });
});

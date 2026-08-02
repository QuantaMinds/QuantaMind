import { describe, it, expect } from "vitest";
import { buildTtftSegments } from "../ttft";

describe("buildTtftSegments", () => {
  it("full stats → load + prefill + remainder summing to TTFT", () => {
    const r = buildTtftSegments(820, { load_ms: 540, prompt_eval_ms: 210, prompt_eval_count: 128 });
    expect(r.available).toBe(true);
    expect(r.segments.map((s) => s.key)).toEqual(["load", "prefill", "remainder"]);
    expect(r.segments.find((s) => s.key === "remainder")?.ms).toBe(70);
    expect(r.total).toBe(820);
    expect(r.promptTokens).toBe(128);
  });

  it("llama-shaped stats (no load) → prefill + remainder, no load segment", () => {
    const r = buildTtftSegments(300, { prompt_eval_ms: 210, prompt_eval_count: 64 });
    expect(r.segments.map((s) => s.key)).toEqual(["prefill", "remainder"]);
    expect(r.segments.find((s) => s.key === "remainder")?.ms).toBe(90);
  });

  it("clamps a negative remainder to zero", () => {
    const r = buildTtftSegments(100, { load_ms: 80, prompt_eval_ms: 60 });
    expect(r.segments.find((s) => s.key === "remainder")?.ms).toBe(0);
  });

  it("no server fields → not available, no segments", () => {
    expect(buildTtftSegments(500, {}).available).toBe(false);
    expect(buildTtftSegments(500, {}).segments).toEqual([]);
    expect(buildTtftSegments(500, undefined).available).toBe(false);
  });

  it("omits the remainder when TTFT is unknown", () => {
    const r = buildTtftSegments(null, { load_ms: 540, prompt_eval_ms: 210 });
    expect(r.segments.map((s) => s.key)).toEqual(["load", "prefill"]);
  });
});

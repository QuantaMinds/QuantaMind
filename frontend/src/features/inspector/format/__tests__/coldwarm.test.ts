import { describe, it, expect } from "vitest";
import { coldWarmSummary, coldWarmState } from "../coldwarm";
import type { HistoryEntry } from "../../../../shared/ipc/workspace/history";

const e = (model: string, load_ms: number | null, ttft_ms: number | null): HistoryEntry => ({
  id: Math.random().toString(), name: "", model, system: "", user: "",
  params: {}, output_preview: "", output_len: 0, token_count: 1,
  load_ms, ttft_ms, tokens_per_sec: 30, ran_at: "t",
});

describe("coldWarmSummary", () => {
  it("returns null until there is both a cold and a warm run", () => {
    expect(coldWarmSummary([e("m", 2400, 13000)], "m")).toBeNull(); // only cold
    expect(coldWarmSummary([e("m", 20, 600)], "m")).toBeNull(); // only warm
  });

  it("splits by load_ms and reports the cold-load TTFT delta", () => {
    const s = coldWarmSummary(
      [e("m", 2400, 13000), e("m", 30, 600), e("m", 10, 700)],
      "m",
    )!;
    expect(s.cold.n).toBe(1);
    expect(s.warm.n).toBe(2);
    expect(s.cold.avgTtftMs).toBe(13000);
    expect(s.warm.avgTtftMs).toBe(650);
    expect(s.deltaTtftMs).toBe(12350);
    // cold load 2400 vs warm avg (30+10)/2=20 → 2380ms (the honest cold-start cost)
    expect(s.deltaLoadMs).toBe(2380);
  });

  it("ignores other models and entries without load_ms", () => {
    const s = coldWarmSummary(
      [e("m", 2400, 13000), e("m", 20, 600), e("other", 9000, 99000), e("m", null, 500)],
      "m",
    )!;
    expect(s.cold.n).toBe(1);
    expect(s.warm.n).toBe(1);
  });
});

describe("coldWarmState", () => {
  it("is 'unsupported' when runs exist but none report load_ms (llama.cpp)", () => {
    const st = coldWarmState([e("m", null, 600), e("m", null, 620), e("m", null, 590)], "m");
    expect(st.kind).toBe("unsupported");
  });

  it("is 'insufficient' with no runs, or with timing but not both cold and warm", () => {
    expect(coldWarmState([], "m").kind).toBe("insufficient");
    expect(coldWarmState([e("m", 30, 600)], "m").kind).toBe("insufficient"); // only warm
  });

  it("is 'ready' once a cold and a warm run exist", () => {
    const st = coldWarmState([e("m", 2400, 13000), e("m", 30, 600)], "m");
    expect(st.kind).toBe("ready");
  });
});

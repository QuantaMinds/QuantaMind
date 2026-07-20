import { describe, expect, it } from "vitest";
import { AgenticStepPayloadSchema, type TrajectoryStep } from "../../../shared/ipc/eval/batch";
import { taskCost } from "../state/taskCost";

const base: TrajectoryStep = {
  run_index: 0,
  step_index: 0,
  raw_output: "{}",
  injection: null,
  kind: "tool_call",
};

describe("taskCost", () => {
  it("sums the reported turn costs and never fabricates an unreported one", () => {
    const cost = taskCost([
      { ...base, prefill_ms: 100, eval_ms: 900, output_tokens: 40, cache_n: 0, prefill_tokens: 30 },
      { ...base, step_index: 1, prefill_ms: 20, eval_ms: 800, output_tokens: 35, cache_n: 60, prefill_tokens: 12 },
      // Synthetic terminal (stall): no model call — contributes nothing, breaks nothing.
      { ...base, step_index: 2, kind: "infinite_loop" },
    ]);
    expect(cost.prefillMsTotal).toBe(120);
    expect(cost.evalMsTotal).toBe(1700);
    expect(cost.outputTokensTotal).toBe(75);
    expect(cost.cacheHitTokensTotal).toBe(60);
    expect(cost.kvTokensMeasured).toBe(true); // cache_n reported → llama.cpp measured tier
    // Peak single-run occupancy: step 1's 60 + 12 + 35 = 107 beats step 0's 0 + 30 + 40 = 70.
    expect(cost.peakContextTokens).toBe(107);
  });

  it("an Ollama run (no cache_n) yields null cache totals — Not available, not zero", () => {
    const cost = taskCost([{ ...base, prefill_ms: 50, eval_ms: 700, output_tokens: 20, prefill_tokens: 34 }]);
    expect(cost.cacheHitTokensTotal).toBeNull();
    expect(cost.kvTokensMeasured).toBe(false); // → the KV tier label must say "estimated"
    expect(cost.peakContextTokens).toBe(54); // occupancy still derivable from prefill+output
  });

  it("no steps → all-null cost (nothing measured, nothing invented)", () => {
    const cost = taskCost([]);
    expect(cost.steps).toBe(0);
    expect(cost.prefillMsTotal).toBeNull();
    expect(cost.peakContextTokens).toBeNull();
    expect(cost.maxStepEndRssBytes).toBeNull();
  });

  it("RSS is a max of step-end samples", () => {
    const cost = taskCost([
      { ...base, resident_bytes: 7_000_000_000 },
      { ...base, step_index: 1, resident_bytes: 7_400_000_000 },
      { ...base, step_index: 2 }, // sample missed — must not read as 0
    ]);
    expect(cost.maxStepEndRssBytes).toBe(7_400_000_000);
  });
});

describe("agentic-step wire drift-guard", () => {
  it("parses the exact payload shape Rust serializes (batch_payloads.rs flatten test)", () => {
    // Mirror of the Rust `agentic_step_payload_carries_env_at_the_top_level_for_the_frontend`
    // fixture — if either side drifts, one of the two tests breaks.
    const wire = {
      collection_id: "easy-coding",
      model: "m",
      task_id: "t",
      is_native: false,
      run_index: 0,
      step_index: 1,
      raw_output: "",
      injection: null,
      kind: "tool_call",
      env: {
        kind: "file_system",
        tree: [{ path: "config.yaml", is_dir: false }],
        focus_path: "config.yaml",
        op: "read",
        content: "timeout: 30",
        matches: [],
      },
      cache_n: 42,
      prefill_tokens: 8,
      prefill_ms: 12,
      eval_ms: 950,
      load_ms: 0,
      total_ms: 1103,
      output_tokens: 40,
      resident_bytes: 7_000_000_000,
    };
    const parsed = AgenticStepPayloadSchema.parse(wire);
    expect(parsed.collection_id).toBe("easy-coding");
    expect(parsed.eval_ms).toBe(950);
    expect(parsed.resident_bytes).toBe(7_000_000_000);
  });

  it("still parses a pre-stamp event (old fixture, no collection_id / turn costs)", () => {
    const old = { model: "m", task_id: "t", run_index: 0, step_index: 0, raw_output: ".", injection: null, kind: "empty_output" };
    const parsed = AgenticStepPayloadSchema.parse(old);
    expect(parsed.collection_id).toBeUndefined();
    expect(parsed.eval_ms).toBeUndefined();
  });
});

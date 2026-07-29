import { describe, it, expect } from "vitest";
import { ModelVerdictSchema } from "../readiness";
import { FailureTrackerSchema } from "../batch";

// Phase 9B widened the readiness IPC: ModelVerdict gained `by_tier` + `failures`, and
// FailureTracker gained `unknown_tool_calls` / `forbidden_calls` / `turn_timeouts`. The
// schema must parse BOTH a current 9B payload and a legacy one (older persisted reports),
// defaulting the new fields rather than failing the parse.
describe("Phase 9B readiness schema", () => {
  it("parses a full 9B ModelVerdict with by_tier + the 7-field failure tracker", () => {
    const raw = {
      model: "gemma-4-12b",
      backend: "ollama",
      verdict: {
        status: "conditional",
        blocking: [],
        conditions: [],
        path: "prompt_based",
        required_tier: "hard",
        cleared_tier: "medium",
      },
      pass_k: 0.31,
      avg_steps: 12.3,
      by_tier: [
        {
          tier: "hard",
          tasks_passed: 1,
          tasks_total: 3,
          avg_steps: 28.0,
          failures: {
            infinite_loop_hits: 2,
            hallucinated_completions: 1,
            malformed_json_calls: 0,
            schema_unrecovered_calls: 0,
            unknown_tool_calls: 5,
            forbidden_calls: 3,
            turn_timeouts: 1,
          },
        },
      ],
      failures: {
        infinite_loop_hits: 2,
        hallucinated_completions: 1,
        malformed_json_calls: 0,
        schema_unrecovered_calls: 0,
        unknown_tool_calls: 5,
        forbidden_calls: 3,
        turn_timeouts: 1,
      },
    };
    const v = ModelVerdictSchema.parse(raw);
    expect(v.by_tier).toBeDefined();
    expect(v.by_tier![0].tier).toBe("hard");
    expect(v.by_tier![0].avg_steps).toBe(28.0);
    expect(v.by_tier![0].failures.forbidden_calls).toBe(3);
    expect(v.failures?.turn_timeouts).toBe(1);
  });

  it("parses a legacy verdict (no by_tier / no failures) and defaults them", () => {
    const legacy = {
      model: "old",
      backend: "ollama",
      verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" },
    };
    const v = ModelVerdictSchema.parse(legacy);
    expect(v.by_tier).toBeUndefined(); // .optional() — absent parses cleanly (deep-dive treats as "no run")
    expect(v.failures).toBeUndefined(); // .optional()
  });

  it("parses a pre-9 FailureTracker that omits the 3 newer fields (absent → treated as 0 by consumers)", () => {
    const ft = FailureTrackerSchema.parse({
      infinite_loop_hits: 1,
      hallucinated_completions: 0,
      malformed_json_calls: 0,
      schema_unrecovered_calls: 0,
    });
    // Optional → absent fields stay undefined; the Failure Taxonomy coalesces them to 0.
    expect(ft.unknown_tool_calls).toBeUndefined();
    expect(ft.forbidden_calls).toBeUndefined();
    expect(ft.turn_timeouts).toBeUndefined();
    expect(ft.infinite_loop_hits).toBe(1); // the original fields still parse as numbers
  });
});

describe("CliffStatus concentration (mirror of Rust CliffConcentration)", () => {
  it("parses old Collapsed records (no concentration) and new ones (with it)", async () => {
    const { CliffStatusSchema } = await import("../readiness");
    // Pre-field persisted record — must keep parsing.
    expect(CliffStatusSchema.parse({ status: "Collapsed", depth: 8845 })).toEqual({ status: "Collapsed", depth: 8845 });
    // New record round-trips the advisory evidence.
    const full = {
      status: "Collapsed",
      depth: 8845,
      concentration: { task_id: "secret", task_failures: 3, total_failures: 4, p_value_milli: 44, holds_without: true },
    };
    expect(CliffStatusSchema.parse(full)).toEqual(full);
  });
});

describe("CliffStatus BudgetLimited (mirror of Rust)", () => {
  it("round-trips the budget-bound outcome distinctly from Collapsed", async () => {
    const { CliffStatusSchema } = await import("../readiness");
    const bl = { status: "BudgetLimited", depth: 8845, cap: 256 };
    expect(CliffStatusSchema.parse(bl)).toEqual(bl);
  });
});

describe("CliffStatus CapMarginal (mirror of Rust)", () => {
  it("round-trips the rung-0 refusal — a config outcome, never a model verdict", async () => {
    const { CliffStatusSchema } = await import("../readiness");
    const cm = { status: "CapMarginal", cap: 256, used_milli: 1000 };
    expect(CliffStatusSchema.parse(cm)).toEqual(cm);
  });
});

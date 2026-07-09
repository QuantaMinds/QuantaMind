import { describe, it, expect } from "vitest";
import { buildRunInput, buildRunOutput } from "../components/runIo";
import type { TaskOutcome, TrajectoryStep } from "../../../shared/ipc/eval/batch";
import type { ToolTask } from "../../../shared/ipc/eval/registry";

const singleTask: ToolTask = {
  id: "weather",
  category: "single",
  prompt: "What's the weather in Paris?",
  tools: [{ name: "get_weather", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "call", name: "get_weather", args: {} },
};

const agenticTask: ToolTask = {
  id: "book-trip",
  category: "agentic",
  prompt: "Book a trip to Tokyo.",
  tools: [{ name: "search_flights", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "no_call" },
  agentic: { mocks: [], end_state: { require_all: [{ tool: "search_flights", args: {} }] } },
};

const singleOutcome = (raw: string): TaskOutcome => ({
  kind: "single",
  passed: raw !== "",
  trace: {
    system_message: "You are a tool-calling model.",
    user_prompt: "What's the weather in Paris?",
    raw_output: raw,
    verdict: { parsed: true, tool_match: true, args_match: true, abstain_correct: null },
  },
});

const step = (run_index: number, step_index: number, raw_output: string, kind: TrajectoryStep["kind"]): TrajectoryStep => ({
  run_index,
  step_index,
  raw_output,
  injection: null,
  kind,
});

describe("buildRunInput", () => {
  it("returns the real captured system message + user line for a single-turn trace (no note)", () => {
    const got = buildRunInput(singleTask, singleOutcome('{"name":"get_weather"}'));
    expect(got).toEqual({
      system: "You are a tool-calling model.",
      user: "What's the weather in Paris?",
      note: null,
    });
  });

  it("reconstructs the agentic prompt package from the task's tools (no note when no decoys)", () => {
    const got = buildRunInput(agenticTask, undefined);
    expect(got.note).toBeNull();
    expect(got.user).toBe("Book a trip to Tokyo.");
    expect(got.system).toContain("Constructed agentic prompt package");
    expect(got.system).toContain("search_flights");
  });

  it("prefers the REAL per-run prompt (step 0's initial_prompt) over the reconstructed template", () => {
    // A generated task re-randomizes its entity ids per run — the static task.prompt template
    // would show the wrong ids for every seed but one. Step 0's initial_prompt is the real thing.
    const steps: TrajectoryStep[] = [
      { ...step(0, 0, '{"name":"search_flights"}', "tool_call"), initial_prompt: "Book a trip to Osaka (renamed)." },
    ];
    const got = buildRunInput(agenticTask, undefined, undefined, steps);
    expect(got.user).toBe("Book a trip to Osaka (renamed).");
    expect(got.system).toContain("Constructed agentic prompt package"); // system is unaffected either way
  });

  it("falls back to the static template when no steps have streamed yet (not-yet-run)", () => {
    const got = buildRunInput(agenticTask, undefined, undefined, []);
    expect(got.user).toBe("Book a trip to Tokyo.");
  });

  it("notes injected decoy tools on an agentic input so the reconstruction isn't passed off as exact", () => {
    const got = buildRunInput(agenticTask, undefined, 3);
    expect(got.note).toContain("3 synthetic decoy tools");
  });

  it("flags a not-yet-run single-turn task with a 'hasn’t been run' note", () => {
    const got = buildRunInput(singleTask, undefined);
    expect(got.user).toBe("What's the weather in Paris?");
    expect(got.note).toContain("hasn’t been run yet");
  });

  it("distinguishes an errored single-turn run from a never-run one in the note", () => {
    const got = buildRunInput(singleTask, { kind: "error", message: "boom" });
    expect(got.note).toContain("errored");
    expect(got.note).not.toContain("hasn’t been run yet");
  });
});

describe("buildRunOutput", () => {
  it("flags a task with no outcome and no streamed steps as not_run", () => {
    expect(buildRunOutput(undefined, [])).toEqual({ state: "not_run" });
  });

  it("renders the in-flight trajectory (not 'not_run') while an agentic run is still streaming", () => {
    const live: TrajectoryStep[] = [step(0, 0, '{"name":"search_flights"}', "tool_call")];
    const got = buildRunOutput(undefined, live);
    expect(got.state).toBe("agentic");
    if (got.state !== "agentic") return;
    expect(got.runs).toHaveLength(1);
    expect(got.runs[0].passed).toBe(false); // no terminal step yet
  });

  it("surfaces an errored run's message", () => {
    const outcome: TaskOutcome = { kind: "error", message: "backend timed out" };
    expect(buildRunOutput(outcome, [])).toEqual({ state: "error", message: "backend timed out" });
  });

  it("returns the raw output for a single-turn run", () => {
    const got = buildRunOutput(singleOutcome('{"name":"get_weather","args":{}}'), []);
    expect(got).toEqual({ state: "single", output: '{"name":"get_weather","args":{}}' });
  });

  it("indicates 'no response' when a single-turn run produced only whitespace", () => {
    const got = buildRunOutput(singleOutcome("   \n  "), []);
    expect(got.state).toBe("empty");
    if (got.state === "empty") expect(got.reason).toContain("no output");
  });

  it("indicates 'no response' for an agentic outcome with no recorded steps", () => {
    const outcome: TaskOutcome = {
      kind: "agentic",
      report: {
        passes: 0,
        total_runs: 0,
        failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        avg_output_tokens_success: null,
        avg_steps: null,
        top_error: "none",
        schema_resilience: null,
      },
    };
    const got = buildRunOutput(outcome, []);
    expect(got.state).toBe("empty");
    if (got.state === "empty") expect(got.reason).toContain("no response");
  });

  it("groups an agentic trajectory by run and marks end-state runs as passed", () => {
    const outcome: TaskOutcome = {
      kind: "agentic",
      report: {
        passes: 1,
        total_runs: 2,
        failures: { infinite_loop_hits: 1, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        avg_output_tokens_success: 50,
        avg_steps: 1.5,
        top_error: "infinite_loop",
        schema_resilience: null,
      },
    };
    const steps: TrajectoryStep[] = [
      step(0, 0, '{"name":"search_flights"}', "tool_call"),
      step(0, 1, "done", "end_state_reached"),
      step(1, 0, '{"name":"search_flights"}', "tool_call"),
      step(1, 1, '{"name":"search_flights"}', "infinite_loop"),
    ];
    const got = buildRunOutput(outcome, steps);
    expect(got.state).toBe("agentic");
    if (got.state !== "agentic") return;
    expect(got.runs).toHaveLength(2);
    expect(got.runs[0]).toMatchObject({ runIndex: 0, passed: true });
    expect(got.runs[0].steps).toHaveLength(2);
    expect(got.runs[1]).toMatchObject({ runIndex: 1, passed: false });
  });
});

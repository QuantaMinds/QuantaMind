import { describe, expect, it } from "vitest";
import { toScoreRows } from "../components/scoreboard/scoreRows";
import type { BatchReport } from "../../../shared/ipc/eval/batch";
import type { InstalledModelInfo } from "../../../shared/ipc/models/storage";

const model = (name: string, quantization: string): InstalledModelInfo =>
  ({ name, quantization, parameter_size: "7B", family: "x", size_bytes: 0, modified_at: "", backend: "ollama" }) as InstalledModelInfo;

describe("toScoreRows", () => {
  it("returns no rows without a report", () => {
    expect(toScoreRows(null, [])).toEqual([]);
  });

  it("formats agentic metrics and shows N/A for nulls, — for single-turn cells", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "qwen",
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 3, tasks_total: 5, passes: 3, total_runs: 5, avg_steps: 2.44, avg_output_tokens_success: 119.6, schema_resilience: null, top_error: "hallucinated", failures: { infinite_loop_hits: 0, hallucinated_completions: 2, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
        {
          model: "loopy",
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 0, tasks_total: 5, passes: 0, total_runs: 5, avg_steps: null, avg_output_tokens_success: null, schema_resilience: null, top_error: "infinite_loop", failures: { infinite_loop_hits: 5, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
      ],
    };
    const rows = toScoreRows(report, [model("qwen", "Q4_K_M")]);

    expect(rows[0]).toMatchObject({ label: "qwen", quant: "Q4_K_M", passK: "3/5", avgSteps: "2.4", effort: "120 tok", topError: "Fake Done" });
    // Unknown model → quant falls back to "—"; null agentic metrics → "N/A".
    expect(rows[1]).toMatchObject({ quant: "—", passK: "0/5", avgSteps: "N/A", effort: "N/A", topError: "Loop Cap" });
  });

  it("shows Tokens/Task (T*) distinct from Effort, and — when nothing completed", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "qwen",
          backend: "ollama",
          toolcall: null,
          // Effort (successes-only mean) 100, but T* (all tokens ÷ completions) 140 — the waste tax.
          agentic: { tasks_passed: 2, tasks_total: 3, passes: 2, total_runs: 3, avg_steps: 2, avg_output_tokens_success: 100, tokens_per_completed: 140, schema_resilience: null, top_error: "hallucinated", failures: { infinite_loop_hits: 0, hallucinated_completions: 1, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
        {
          model: "loopy", // nothing completed → T* is null → "—", never a fabricated 0
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 0, tasks_total: 2, passes: 0, total_runs: 2, avg_steps: 3, avg_output_tokens_success: null, tokens_per_completed: null, schema_resilience: null, top_error: "infinite_loop", failures: { infinite_loop_hits: 2, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
      ],
    };
    const rows = toScoreRows(report, []);
    expect(rows[0]).toMatchObject({ effort: "100 tok", tokensPerTask: "140 tok" });
    expect(rows[1]).toMatchObject({ tokensPerTask: "N/A" }); // null T* → fmtTokens → N/A
  });

  it("maps schema resilience as a percent (— when null) and the top error label", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "qwen", // a measured resilience + a schema top error
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 4, tasks_total: 5, passes: 4, total_runs: 5, avg_steps: 2, avg_output_tokens_success: 100, schema_resilience: 0.5, top_error: "malformed_schema", failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 1 } },
          error: null,
        },
        {
          model: "clean", // no schema errors seen
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 5, tasks_total: 5, passes: 5, total_runs: 5, avg_steps: 1, avg_output_tokens_success: 80, schema_resilience: null, top_error: "none", failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
      ],
    };
    const rows = toScoreRows(report, []);
    expect(rows[0]).toMatchObject({ schemaResil: "50%", topError: "Bad Schema" });
    expect(rows[1]).toMatchObject({ schemaResil: "—" });
  });

  it("shows — for steps/effort when the column has no agentic tasks", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "m",
          backend: "ollama",
          toolcall: { n: 3, parse_rate: 1, tool_selection_acc: 1, arg_acc: 1, abstain_acc: null, composite: 0.92, prompt_tokens: null, per_task: [] },
          agentic: null,
          error: null,
        },
      ],
    };
    const rows = toScoreRows(report, []);
    // Single-turn columns now surface the composite as the Pass cell (not "—"),
    // so the matrix is meaningful for non-agentic collections too.
    expect(rows[0]).toMatchObject({ passK: "92%", avgSteps: "—", effort: "—", topError: "—", composite: "92%" });
  });

  it("surfaces native (Tool-Calling) avg steps separately from the prompt avg steps", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "qwen",
          backend: "ollama",
          toolcall: null,
          agentic: { tasks_passed: 1, tasks_total: 2, passes: 1, total_runs: 2, avg_steps: 3.2, avg_output_tokens_success: 100, schema_resilience: null, top_error: "none", failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          agentic_native_fc: { tasks_passed: 2, tasks_total: 2, passes: 2, total_runs: 2, avg_steps: 1.5, avg_output_tokens_success: 60, schema_resilience: null, top_error: "none", failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
      ],
    };
    const rows = toScoreRows(report, []);
    // Prompt steps and native steps are NOT conflated — each pass keeps its own cost.
    expect(rows[0]).toMatchObject({ avgSteps: "3.2", avgStepsNative: "1.5", passKNative: "2/2" });
  });

  it("an all-errored native pass reads as a FAILURE, not a misleading '0/0' (broken template)", () => {
    const nat = (native_error_class: "schema_rejected" | "infra_host") => ({
      tasks_passed: 0, tasks_total: 0, passes: 0, total_runs: 0, avg_steps: null,
      avg_output_tokens_success: null, schema_resilience: null, top_error: "none" as const,
      failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
      tasks_errored: 3, native_error_class,
    });
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        { model: "qwen-gguf", backend: "llama_cpp", toolcall: null, agentic: null, agentic_native_fc: nat("schema_rejected"), error: null },
        { model: "qwen-ollama", backend: "ollama", toolcall: null, agentic: null, agentic_native_fc: nat("infra_host"), error: null },
      ],
    };
    const rows = toScoreRows(report, []);
    // Schema-rejected (the template/model can't do native tools) → honest capability failure.
    expect(rows[0].passKNative).toBe("Native unsupported");
    expect(rows[0].topErrorNative).toBe("Native unsupported");
    // Infra/host error → blamed on the machine, not the model.
    expect(rows[1].passKNative).toBe("Backend error");
    // Neither is the old green "0/0".
    expect(rows[0].passKNative).not.toBe("0/0");
    expect(rows[1].passKNative).not.toBe("0/0");
  });
});

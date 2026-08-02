import { describe, expect, it } from "vitest";
import { batchToCsv } from "../exportBatch";
import type { BatchReport } from "../../../shared/ipc/eval/batch";

describe("batchToCsv", () => {
  it("emits a header + one row per model, preserving N/A", () => {
    const report: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "qwen",
          backend: "llama_cpp",
          toolcall: null,
          agentic: { tasks_passed: 3, tasks_total: 5, passes: 3, total_runs: 5, avg_steps: 2.4, avg_output_tokens_success: 120, schema_resilience: null, top_error: "hallucinated", failures: { infinite_loop_hits: 0, hallucinated_completions: 2, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
        {
          model: "loopy",
          backend: "llama_cpp",
          toolcall: null,
          agentic: { tasks_passed: 0, tasks_total: 5, passes: 0, total_runs: 5, avg_steps: null, avg_output_tokens_success: null, schema_resilience: null, top_error: "infinite_loop", failures: { infinite_loop_hits: 5, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } },
          error: null,
        },
      ],
    };
    const csv = batchToCsv(report, []);
    const lines = csv.split("\n");
    expect(lines[0]).toBe("Model,Quant,Pass^k,Avg Steps,Effort,Top Error,Composite");
    expect(lines[1]).toBe("qwen,—,3/5,2.4,120 tok,Fake Done,—");
    expect(lines[2]).toContain("N/A"); // the loopy row keeps N/A, never a fake 0
  });
});

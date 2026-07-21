import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MatrixScoreboard } from "../components/scoreboard/MatrixScoreboard";
import { useEvalRegistryStore } from "../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBatchStore, flushBatchBufferForTests } from "../state/batchStore";
import type { BatchProgress } from "../../../shared/ipc/eval/batch";
import type { ToolTask } from "../../../shared/ipc/eval/registry";

const MODEL = "llama3.2:3b";
const task: ToolTask = {
  id: "weather",
  category: "single",
  prompt: "p",
  tools: [{ name: "get_weather", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "call", name: "get_weather", args: {} },
};

beforeEach(() => {
  useBatchStore.getState().reset();
  useEvalRegistryStore.setState({ tasks: [task] });
  useInstalledModelsStore.setState({
    list: [{ name: MODEL, size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "Q4_K_M", backend: "ollama" }],
    status: "ready",
    error: null,
    lastRefreshedAt: 1,
  });
});

describe("MatrixScoreboard (Simulator) data flow", () => {
  it("renders Pass + the aggregate from a real task_done event — not a blank '—'", () => {
    const s = useBatchStore.getState();
    s.startRun();
    s.ingestProgress({ phase: "started", model: MODEL, task_id: "weather", index: 0, total: 1, category: "single" } as BatchProgress);
    s.ingestProgress({
      phase: "done",
      model: MODEL,
      task_id: "weather",
      outcome: {
        kind: "single",
        passed: true,
        trace: {
          system_message: "sys",
          user_prompt: "u",
          raw_output: '{"name":"get_weather","args":{}}',
          verdict: { parsed: true, tool_match: true, args_match: true, abstain_correct: null },
          prompt_tokens: 10,
        },
      },
    } as BatchProgress);
    flushBatchBufferForTests();

    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={"weather"} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);

    const row = screen.getByTestId("scoreboard-row-weather");
    expect(row).toHaveTextContent("Pass"); // the Result badge renders, not a dash
    // The aggregate proves the (model,task) outcome actually landed in the store.
    expect(screen.getByTestId("matrix-scoreboard")).toHaveTextContent("100% Pass Rate");
  });

  it("shows an amber Partial badge for a k>1 agentic outcome that passed some-but-not-all runs", () => {
    const s = useBatchStore.getState();
    s.startRun();
    s.ingestProgress({ phase: "started", model: MODEL, task_id: "weather", index: 0, total: 1, category: "agentic" } as BatchProgress);
    s.ingestProgress({
      phase: "done",
      model: MODEL,
      task_id: "weather",
      outcome: {
        kind: "agentic",
        report: {
          passes: 3,
          total_runs: 5,
          avg_steps: 2.0,
          avg_output_tokens_success: 80,
          schema_resilience: null,
          top_error: "none",
          failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        },
      },
    } as BatchProgress);
    flushBatchBufferForTests();

    render(<MatrixScoreboard model={MODEL} k={5} maxSteps={8} focusedTaskId={"weather"} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    const result = screen.getByTestId("result-weather");
    // 3/5 is "Unreliable", not a flat Fail — aligns the badge with the Pass^k fraction.
    expect(result).toHaveTextContent("Partial 3/5");
    expect(result).not.toHaveTextContent("Fail");
  });

  it("echoes the Phase 9 run shape as header chips (Tier · K · Decoys)", () => {
    render(
      <MatrixScoreboard model={MODEL} k={16} maxSteps={8} tierLabel="Hard" decoys={3} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />,
    );
    const chips = screen.getByTestId("scoreboard-run-chips");
    expect(chips).toHaveTextContent("Tier: Hard");
    expect(chips).toHaveTextContent("K: 16");
    expect(chips).toHaveTextContent("Decoys: 3");
  });

  it("reads 'Decoys: off' when no decoy budget is set", () => {
    render(<MatrixScoreboard model={MODEL} k={8} maxSteps={8} tierLabel="Medium" focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    expect(screen.getByTestId("scoreboard-run-chips")).toHaveTextContent("Decoys: off");
  });

  it("scopes the board to a single task when runTaskId is set (only that row + a Task chip)", () => {
    const second: ToolTask = { ...task, id: "forecast" };
    useEvalRegistryStore.setState({ tasks: [task, second] });
    // No filter → both rows render.
    const { rerender } = render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    expect(screen.getByTestId("scoreboard-row-weather")).toBeInTheDocument();
    expect(screen.getByTestId("scoreboard-row-forecast")).toBeInTheDocument();
    // Scoped to "weather" → only that row, and the header names the single-task scope.
    rerender(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={"weather"} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} runTaskId="weather" />);
    expect(screen.getByTestId("scoreboard-row-weather")).toBeInTheDocument();
    expect(screen.queryByTestId("scoreboard-row-forecast")).toBeNull();
    expect(screen.getByTestId("scoreboard-single-task")).toHaveTextContent("Task: weather");
  });

  it("ignores a stale runTaskId not in this collection (shows the collection, not a blank board)", () => {
    useEvalRegistryStore.setState({ tasks: [task] });
    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} runTaskId="ghost" />);
    expect(screen.getByTestId("scoreboard-row-weather")).toBeInTheDocument();
    expect(screen.queryByTestId("scoreboard-single-task")).toBeNull();
  });

  it("shows a separate Native column from a native task_done and opens the native trace on click", () => {
    const s = useBatchStore.getState();
    s.startRun();
    // A NATIVE per-task result (is_native) — the native pass posts its own column.
    s.ingestProgress({
      phase: "done",
      model: MODEL,
      task_id: "weather",
      is_native: true,
      outcome: {
        kind: "agentic",
        report: {
          passes: 1,
          total_runs: 1,
          avg_steps: 2.0,
          avg_output_tokens_success: 40,
          schema_resilience: null,
          top_error: "none",
          failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        },
      },
    } as BatchProgress);
    flushBatchBufferForTests();

    const setPass = vi.fn();
    render(
      <MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={"weather"} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={setPass} />,
    );
    // The Tool-Calling (native) column + its pass/fail cell appear (separate from Prompt-based).
    const nativeCell = screen.getByTestId("native-cell-weather");
    expect(nativeCell).toHaveTextContent("Pass");
    expect(screen.getByTestId("scoreboard-table")).toHaveTextContent("Tool-Calling");
    // Clicking the native result opens the NATIVE trace in the Evaluator.
    fireEvent.click(nativeCell);
    expect(setPass).toHaveBeenCalledWith("native");
  });

  it("populates the Tool-Calling (native) AGGREGATE line from a native-only run (was a blank '—')", () => {
    const s = useBatchStore.getState();
    s.startRun();
    // A NATIVE-ONLY run: the outcome lands in nativeOutcomeByKey, NOT outcomeByKey. The
    // AGGREGATE used to read only the prompt pass, so it showed "—"; it now reads per-pass.
    s.ingestProgress({
      phase: "done",
      model: MODEL,
      task_id: "weather",
      is_native: true,
      outcome: {
        kind: "agentic",
        report: {
          passes: 4,
          total_runs: 5,
          avg_steps: 2.5,
          avg_output_tokens_success: 40,
          schema_resilience: null,
          top_error: "none",
          failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        },
      },
    } as BatchProgress);
    flushBatchBufferForTests();

    render(<MatrixScoreboard model={MODEL} k={5} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="native" setFocusedPass={() => {}} />);
    // The native AGGREGATE line shows real numbers (4/5 = 80%, avg steps 2.5) — not "—".
    const native = screen.getByTestId("aggregate-native");
    expect(native).toHaveTextContent("Tool-Calling (native):");
    expect(native).toHaveTextContent("80% Pass Rate");
    expect(native).toHaveTextContent("2.5");
    // A native-only run shows no Prompt-based aggregate line.
    expect(screen.queryByTestId("aggregate-prompt")).toBeNull();
  });

  it("derives Target Tool from an agentic end-state and shows TC Steps from the native run", () => {
    const agenticTask: ToolTask = {
      id: "pr_task",
      category: "agentic",
      prompt: "Open a PR.",
      tools: [{ name: "open_pr", description: "", parameters: { type: "object", properties: {} } }],
      expected: { type: "no_call" },
      agentic: { mocks: [], end_state: { require_all: [{ tool: "open_pr", args: {} }] } },
    };
    useEvalRegistryStore.setState({ tasks: [agenticTask] });
    const s = useBatchStore.getState();
    s.startRun();
    s.ingestProgress({
      phase: "done",
      model: MODEL,
      task_id: "pr_task",
      is_native: true,
      outcome: {
        kind: "agentic",
        report: {
          passes: 1, total_runs: 1, avg_steps: 1.5, avg_output_tokens_success: 40, schema_resilience: null,
          top_error: "none",
          failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
        },
      },
    } as BatchProgress);
    flushBatchBufferForTests();

    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    // Target Tool is no longer a blank "—" for an agentic task — it names the end-state tool.
    expect(screen.getByTestId("scoreboard-row-pr_task")).toHaveTextContent("open_pr");
    // TC Steps shows the native run's avg steps; this is a NATIVE-ONLY run so the Prompt-based /
    // PB Steps columns are hidden entirely (no empty prompt cells).
    expect(screen.getByTestId("native-steps-pr_task")).toHaveTextContent("1.5");
    expect(screen.queryByTestId("prompt-steps-pr_task")).toBeNull();
  });

  it("derives Target Tool for a web-UI (require_end_state) task from its action tools, minus the decoy", () => {
    // Regression: a state-diff task has no tool checkpoints, so Target Tool was blank "—". It now
    // names the action tools the model drives the UI with (fill/submit), excluding the must-not-call
    // decoy (delete_account).
    const webUiTask: ToolTask = {
      id: "es_wu_apply_coupon",
      category: "agentic",
      prompt: "Apply the coupon and submit.",
      tools: [
        { name: "fill", description: "", parameters: { type: "object", properties: {} } },
        { name: "submit", description: "", parameters: { type: "object", properties: {} } },
        { name: "delete_account", description: "", parameters: { type: "object", properties: {} } },
      ],
      expected: { type: "no_call" },
      agentic: { mocks: [], end_state: { require_end_state: { submitted: true } }, must_not_call: ["delete_account"] },
    };
    useEvalRegistryStore.setState({ tasks: [webUiTask] });
    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    const row = screen.getByTestId("scoreboard-row-es_wu_apply_coupon");
    expect(row).toHaveTextContent("fill, submit");
    expect(row).not.toHaveTextContent("delete_account");
  });

  it("the Action column is gone — the result cells themselves open the trace", () => {
    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    expect(screen.getByTestId("scoreboard-table")).not.toHaveTextContent("View Trace");
    expect(screen.getByTestId("scoreboard-table")).not.toHaveTextContent("Action");
  });

  it("collapses and expands the card, showing a 'click to expand' summary instead of blank", () => {
    render(<MatrixScoreboard model={MODEL} k={1} maxSteps={8} focusedTaskId={null} setFocusedTaskId={() => {}} focusedPass="prompt" setFocusedPass={() => {}} />);
    // Body (the task table area / footnote) visible by default; no collapsed summary.
    expect(screen.getByTestId("matrix-scoreboard")).toHaveTextContent("AGGREGATE");
    expect(screen.queryByTestId("simulator-collapsed-summary")).toBeNull();

    fireEvent.click(screen.getByTestId("simulator-collapse"));
    // Body gone, but the header now shows a summary + an expand cue (never blank).
    expect(screen.getByTestId("matrix-scoreboard")).not.toHaveTextContent("AGGREGATE");
    expect(screen.getByTestId("simulator-collapsed-summary")).toHaveTextContent("click to expand");

    fireEvent.click(screen.getByTestId("simulator-collapse"));
    expect(screen.getByTestId("matrix-scoreboard")).toHaveTextContent("AGGREGATE"); // expands back
  });
});

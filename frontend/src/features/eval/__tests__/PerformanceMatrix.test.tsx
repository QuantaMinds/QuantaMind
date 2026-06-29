import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockRejectedValue(new Error("no backend in test")) }));

import { PerformanceMatrix } from "../components/scoreboard/PerformanceMatrix";
import { useBatchStore } from "../state/batchStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useCliffStore } from "../state/cliffStore";
import type { BatchReport } from "../../../shared/ipc/eval/batch";

const report: BatchReport = {
  collection_id: "c",
  columns: [
    { model: "qwen", backend: "ollama", toolcall: null, agentic: { tasks_passed: 5, tasks_total: 5, passes: 5, total_runs: 5, avg_steps: 2.4, avg_output_tokens_success: 112, schema_resilience: null, top_error: "none", failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } }, error: null },
    { model: "loopy", backend: "ollama", toolcall: null, agentic: { tasks_passed: 1, tasks_total: 5, passes: 1, total_runs: 5, avg_steps: null, avg_output_tokens_success: null, schema_resilience: null, top_error: "infinite_loop", failures: { infinite_loop_hits: 4, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 } }, error: null },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  useBatchStore.getState().reset();
  useCliffStore.getState().reset();
  useCliffStore.setState({ results: {}, probed: {}, brokenBaseline: {}, request: null });
  useInstalledModelsStore.setState({ list: [], status: "ready", error: null, lastRefreshedAt: 1 });
});

describe("PerformanceMatrix", () => {
  it("renders one row per model with N/A preserved, and focuses on row click", () => {
    useBatchStore.setState({ report });
    const onFocus = vi.fn();
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={onFocus} />);

    expect(screen.getByTestId("matrix-model-row-qwen")).toBeInTheDocument();
    expect(screen.getByTestId("matrix-model-row-loopy")).toHaveTextContent("N/A"); // null steps/effort

    fireEvent.click(screen.getByTestId("matrix-model-row-loopy"));
    expect(onFocus).toHaveBeenCalledWith("loopy");

    // The Driver-B/D reliability columns are present.
    expect(screen.getByRole("columnheader", { name: "Schema Resil." })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Context Limit" })).toBeInTheDocument();
  });

  it("pre-fills the cliff request (model + collection) and navigates to Audit — never auto-runs", async () => {
    const { useNavStore } = await import("../../../shared/state/navStore");
    useBatchStore.setState({ report });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);

    const link = screen.getByTestId("cliff-run-qwen");
    expect(link).toHaveTextContent("Run probe");
    fireEvent.click(link);

    // The probe is PRE-FILLED for this model + the report's collection, then we
    // switch tabs — the panel waits for an explicit Run (guardrail 1).
    expect(useCliffStore.getState().request).toMatchObject({ model: "qwen", backend: "ollama", collectionId: "c", steps: 5 });
    expect(useNavStore.getState().topView).toBe("audit");
  });

  it("shows the measured cliff depth from the backend store instead of the Run-probe link", () => {
    useBatchStore.setState({ report });
    useCliffStore.setState({ results: { c: { qwen: 12000 } } });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByTestId("cliff-value-qwen")).toHaveTextContent("12,000 tok");
    expect(screen.queryByTestId("cliff-run-qwen")).toBeNull(); // measured → no link
  });

  it("a measured cell offers a re-probe (↻) that pre-fills + opens Audit", async () => {
    const { useNavStore } = await import("../../../shared/state/navStore");
    useBatchStore.setState({ report });
    useCliffStore.setState({ results: { c: { qwen: 12000 } } }); // already measured
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    fireEvent.click(screen.getByTestId("cliff-reprobe-qwen"));
    expect(useCliffStore.getState().request).toMatchObject({ model: "qwen", collectionId: "c", steps: 5 });
    expect(useNavStore.getState().topView).toBe("audit");
  });

  it("shows '✓ no cliff' for a model that was probed but found no cliff (not a misleading Run-probe)", () => {
    useBatchStore.setState({ report });
    useCliffStore.setState({ probed: { c: { qwen: true } } }); // probed, accuracy held
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByTestId("cliff-nocliff-qwen")).toHaveTextContent("no cliff");
    expect(screen.queryByTestId("cliff-run-qwen")).toBeNull(); // probed → no "Run probe" link
  });

  it("shows 'fails from start' (not '✓ no cliff') when the probe baseline is broken", () => {
    useBatchStore.setState({ report });
    // Probed AND broken baseline (0% at the smallest context) → must be the red failure
    // state, never the green "✓ no cliff".
    useCliffStore.setState({ probed: { c: { qwen: true } }, brokenBaseline: { c: { qwen: true } } });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByTestId("cliff-broken-qwen")).toHaveTextContent("fails from start");
    expect(screen.queryByTestId("cliff-nocliff-qwen")).toBeNull(); // never claim "no cliff" when broken
    expect(screen.queryByTestId("cliff-run-qwen")).toBeNull();
  });

  it("broken baseline wins over a persisted depth — 'fails from start', not the number", () => {
    useBatchStore.setState({ report });
    // The backend persists broken-baseline as a collapse depth (for the Agent Report
    // gate), so results AND brokenBaseline can both be set — the cell must prioritize
    // the red failure, never render the misleading depth.
    useCliffStore.setState({ results: { c: { qwen: 388 } }, probed: { c: { qwen: true } }, brokenBaseline: { c: { qwen: true } } });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByTestId("cliff-broken-qwen")).toHaveTextContent("fails from start");
    expect(screen.queryByTestId("cliff-value-qwen")).toBeNull(); // the "388 tok" depth is suppressed
  });

  it("shows the 'click a row to inspect' hint only with 2+ models", () => {
    useBatchStore.setState({ report }); // two models
    const { rerender } = render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByText(/click a row to inspect/)).toBeInTheDocument();

    useBatchStore.setState({ report: { collection_id: "c", columns: [report.columns[0]] } }); // one model
    rerender(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.queryByText(/click a row to inspect/)).toBeNull();
  });

  it("renders an always-visible legend explaining Context Limit + the probe payoff", () => {
    useBatchStore.setState({ report });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    const legend = screen.getByTestId("matrix-legend");
    expect(legend).toHaveTextContent(/Context Limit/);
    expect(legend).toHaveTextContent(/Agent-Readiness verdict/);
  });

  it("prompts to run when there is no report yet", () => {
    render(<PerformanceMatrix focusedModel="" onFocusModel={() => {}} />);
    expect(screen.queryByTestId("performance-matrix-table")).toBeNull();
    expect(screen.getByText(/Run Batch to compare/i)).toBeInTheDocument();
  });

  it("surfaces the full failure breakdown (incl. the 2 previously-hidden counts) on the Top Error cell", () => {
    useBatchStore.setState({ report });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    // loopy hit the loop cap 4×. Hovering the ⓘ opens a clip-safe portal tooltip
    // (replacing the WebView-unreliable native title) exposing ALL four counts —
    // including Fake Done / Bad Schema, which the badge itself hides.
    const info = screen.getByTestId("failbreak-loopy");
    expect(screen.queryByTestId("tooltip-failbreak-loopy")).toBeNull(); // closed until hover
    fireEvent.mouseEnter(info);
    const tip = screen.getByTestId("tooltip-failbreak-loopy");
    expect(tip).toHaveTextContent(/Loop Cap 4/);
    expect(tip).toHaveTextContent(/Fake Done 0/);
    expect(tip).toHaveTextContent(/Bad Schema 0/);
    expect(tip).toHaveTextContent(/Malformed 0/);
    fireEvent.mouseLeave(info);
    expect(screen.queryByTestId("tooltip-failbreak-loopy")).toBeNull(); // closes on leave
    // qwen had zero failures → no breakdown affordance.
    expect(screen.queryByTestId("failbreak-qwen")).toBeNull();
  });

  it("shows the Bad Dialect badge AND its breakdown for a foreign-dialect-only model", () => {
    // Regression: a model whose only failures are foreign_dialect used to show the badge but
    // a `total === 0` breakdown (the ⓘ vanished), reading as 'not wired'. The terminal mode
    // must count toward the total and appear in the tooltip.
    const dialectReport: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "gemmaqat",
          backend: "ollama",
          toolcall: null,
          agentic: {
            tasks_passed: 0, tasks_total: 3, passes: 0, total_runs: 3, avg_steps: 1, avg_output_tokens_success: null,
            schema_resilience: null, top_error: "foreign_dialect",
            failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0, foreign_dialect_calls: 3 },
          },
          error: null,
        },
      ],
    };
    useBatchStore.setState({ report: dialectReport });
    render(<PerformanceMatrix focusedModel="gemmaqat" onFocusModel={() => {}} />);
    expect(screen.getByTestId("matrix-model-row-gemmaqat")).toHaveTextContent("Bad Dialect");
    const info = screen.getByTestId("failbreak-gemmaqat"); // ⓘ present → total > 0
    fireEvent.mouseEnter(info);
    expect(screen.getByTestId("tooltip-failbreak-gemmaqat")).toHaveTextContent(/Bad Dialect 3/);
  });

  it("shows the No Output badge AND its breakdown for an empty-output-only model", () => {
    // The gemma-qat prompt-path symptom: the model emits nothing usable ('.') → empty_output,
    // NOT hallucinated. Badge + ⓘ must both reflect it (not a silent '0 failures').
    const emptyReport: BatchReport = {
      collection_id: "c",
      columns: [
        {
          model: "gemmaqat",
          backend: "ollama",
          toolcall: null,
          agentic: {
            tasks_passed: 0, tasks_total: 3, passes: 0, total_runs: 3, avg_steps: 1, avg_output_tokens_success: null,
            schema_resilience: null, top_error: "empty_output",
            failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0, empty_output_calls: 3 },
          },
          error: null,
        },
      ],
    };
    useBatchStore.setState({ report: emptyReport });
    render(<PerformanceMatrix focusedModel="gemmaqat" onFocusModel={() => {}} />);
    expect(screen.getByTestId("matrix-model-row-gemmaqat")).toHaveTextContent("No Output");
    const info = screen.getByTestId("failbreak-gemmaqat");
    fireEvent.mouseEnter(info);
    expect(screen.getByTestId("tooltip-failbreak-gemmaqat")).toHaveTextContent(/No Output 3/);
  });

  const failures = { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 };
  const nativeReport: BatchReport = {
    collection_id: "c",
    columns: [
      {
        model: "qwen",
        backend: "ollama",
        toolcall: null,
        agentic: { tasks_passed: 5, tasks_total: 5, passes: 5, total_runs: 5, avg_steps: 2.4, avg_output_tokens_success: 112, schema_resilience: null, top_error: "none", failures },
        agentic_native_fc: { tasks_passed: 2, tasks_total: 5, passes: 2, total_runs: 5, avg_steps: 3.0, avg_output_tokens_success: 90, schema_resilience: null, top_error: "hallucinated", failures: { ...failures, hallucinated_completions: 3 } },
        error: null,
      },
    ],
  };

  it("gives a model TWO rows — Tool-Calling (native) + Prompt-based — when native was measured", () => {
    useBatchStore.setState({ report: nativeReport });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);

    // Method column instead of a Show/Hide toggle; one row per pass.
    expect(screen.getByRole("columnheader", { name: "Method" })).toBeInTheDocument();
    expect(screen.queryByTestId("matrix-native-toggle")).toBeNull();
    // Two rows for the one model.
    expect(screen.getByTestId("matrix-native-row-qwen")).toHaveTextContent("Tool-Calling");
    expect(screen.getByTestId("matrix-model-row-qwen")).toHaveTextContent("Prompt-based");
    // Each row holds ITS pass's Pass^k.
    expect(screen.getByTestId("matrix-native-qwen")).toHaveTextContent("2/5"); // native row
    expect(screen.getByTestId("matrix-prompt-qwen")).toHaveTextContent("5/5"); // prompt row
  });

  it("gives only the Prompt-based row when native was NOT measured (no native row)", () => {
    const mixed: BatchReport = {
      collection_id: "c",
      columns: [
        { model: "tinyllama.gguf", backend: "llama_cpp", toolcall: null, agentic: { tasks_passed: 3, tasks_total: 5, passes: 3, total_runs: 5, avg_steps: 2, avg_output_tokens_success: 80, schema_resilience: null, top_error: "none", failures }, agentic_native_fc: null, error: null },
      ],
    };
    useBatchStore.setState({ report: mixed });
    render(<PerformanceMatrix focusedModel="tinyllama.gguf" onFocusModel={() => {}} />);
    // Prompt-based row present; NO Tool-Calling row for an unmeasured-native model.
    expect(screen.getByTestId("matrix-model-row-tinyllama.gguf")).toHaveTextContent("Prompt-based");
    expect(screen.queryByTestId("matrix-native-row-tinyllama.gguf")).toBeNull();
    expect(screen.queryByTestId("matrix-native-tinyllama.gguf")).toBeNull();
  });

  it("gives only the Tool-Calling row for a NATIVE-ONLY run (no empty Prompt-based row)", () => {
    // The user picked Tool-Calling only → the column has native data but no prompt agentic/toolcall.
    const nativeOnly: BatchReport = {
      collection_id: "c",
      columns: [
        { model: "qwen", backend: "ollama", toolcall: null, agentic: null, error: null,
          agentic_native_fc: { tasks_passed: 2, tasks_total: 2, passes: 2, total_runs: 2, avg_steps: 1.5, avg_output_tokens_success: 60, schema_resilience: null, top_error: "none", failures } },
      ],
    };
    useBatchStore.setState({ report: nativeOnly });
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    expect(screen.getByTestId("matrix-native-row-qwen")).toHaveTextContent("Tool-Calling");
    expect(screen.getByTestId("matrix-native-qwen")).toHaveTextContent("2/2");
    // No empty Prompt-based row for a native-only run.
    expect(screen.queryByTestId("matrix-model-row-qwen")).toBeNull();
  });

  it("shows only Prompt-based rows + a hint when native was not measured for any model", () => {
    useBatchStore.setState({ report }); // no agentic_native_fc on any column
    render(<PerformanceMatrix focusedModel="qwen" onFocusModel={() => {}} />);
    // The hint explains the missing native measurement; no native rows exist.
    expect(screen.getByTestId("native-fc-empty-hint")).toHaveTextContent(/Measure native tool-calling/i);
    expect(screen.queryByTestId("matrix-native-row-qwen")).toBeNull();
  });
});

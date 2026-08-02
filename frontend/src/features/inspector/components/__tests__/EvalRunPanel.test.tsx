import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("../../../../shared/ipc/system/inspect", () => ({
  inspectModel: vi.fn().mockResolvedValue({ dims: null }),
  estimateKvCacheBytes: vi.fn().mockResolvedValue(2_000_000_000),
  contextCeilings: vi.fn().mockResolvedValue({ f16: 8192, q8: 16384, q4: 32768 }),
}));
vi.mock("../../../../shared/ipc/compare/hardware", () => ({
  getHardwareSnapshot: vi.fn().mockResolvedValue(null),
}));

import { EvalRunPanel } from "../evalrun/EvalRunPanel";
import { useBatchStore, cellKey } from "../../../eval/state/batchStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import type { TrajectoryStep } from "../../../../shared/ipc/eval/batch";

const step = (over: Partial<TrajectoryStep>): TrajectoryStep => ({
  run_index: 0,
  step_index: 0,
  raw_output: "{}",
  injection: null,
  kind: "tool_call",
  ...over,
});

beforeEach(() => {
  useBatchStore.getState().reset();
  useInstalledModelsStore.setState({ list: [] });
});

describe("EvalRunPanel", () => {
  it("shows the run-a-test hint when no batch data exists", () => {
    render(<EvalRunPanel />);
    expect(screen.getByTestId("eval-run-empty")).toBeInTheDocument();
  });

  it("renders a per-task card keyed to the collection, with llama.cpp cache truth", () => {
    useBatchStore.setState({
      collectionId: "easy-coding",
      tasksByModel: { "qwen2.5:3b": ["t1"] },
      stepsByKey: {
        [cellKey("qwen2.5:3b", "t1")]: [
          step({ prefill_ms: 100, eval_ms: 900, output_tokens: 40, cache_n: 60, prefill_tokens: 12 }),
        ],
      },
      outcomeByKey: {},
    });
    render(<EvalRunPanel />);
    expect(screen.getByTestId("eval-run-model-qwen2.5:3b")).toBeInTheDocument();
    expect(screen.getByText("easy-coding")).toBeInTheDocument();
    expect(screen.getByTestId("eval-task-card-t1")).toBeInTheDocument();
    expect(screen.getByText("60 tok")).toBeInTheDocument(); // measured cache hits
    // Every surface explains itself: page-, memory- and card-level info buttons.
    expect(screen.getByTestId("info-eval-run")).toBeInTheDocument();
    expect(screen.getByTestId("info-eval-memory")).toBeInTheDocument();
    expect(screen.getByTestId("info-task-metrics-t1")).toBeInTheDocument();
  });

  it("a run on a backend without cache reporting shows Not available — never a fabricated 0", () => {
    useBatchStore.setState({
      collectionId: "mcp:byo",
      tasksByModel: { m: ["t1"] },
      stepsByKey: { [cellKey("m", "t1")]: [step({ prefill_ms: 50, eval_ms: 700, output_tokens: 20, prefill_tokens: 34 })] },
      outcomeByKey: {},
    });
    render(<EvalRunPanel />);
    const card = screen.getByTestId("eval-task-card-t1");
    expect(card.textContent).toContain("N/A"); // the honest gap marker (restyled from "Not available")
    expect(card.textContent).not.toContain("0 tok");
  });

  it("a NATIVE-only run renders its trajectory — the live-smoke regression", () => {
    // Reproduces the 2026-07-20 smoke finding: a native-FC run streams ONLY into the
    // native slices; the panel showed 'Running… · 0 steps' + all-Not-available because
    // it read the prompt slices alone. Native data must render, tagged as native.
    useBatchStore.setState({
      collectionId: "easy-research-search",
      tasksByModel: { "qwen3.5:9b": ["es_rs_search_fact"] },
      stepsByKey: {},
      outcomeByKey: {},
      nativeStepsByKey: {
        [cellKey("qwen3.5:9b", "es_rs_search_fact")]: [
          // Field values from the real on-disk transcript of that smoke run.
          step({ prefill_tokens: 482, prefill_ms: 3947, eval_ms: 7145, load_ms: 6376, total_ms: 17701, output_tokens: 74, resident_bytes: 8_023_867_392 }),
        ],
      },
      nativeOutcomeByKey: {},
    });
    render(<EvalRunPanel />);
    const card = screen.getByTestId("eval-task-card-es_rs_search_fact-native");
    expect(card.textContent).toContain("Native FC");
    expect(card.textContent).toContain("74"); // output tokens actually shown
    expect(card.textContent).not.toContain("0 runs · 0 steps");
    // The memory panel takes its RSS peak across BOTH passes.
    expect(screen.getByTestId("eval-memory-panel").textContent).toContain("7.5GB");
  });

  it("an OOM task error surfaces the red OOM badge and the ceiling answer block", () => {
    useBatchStore.setState({
      collectionId: "hard-support",
      tasksByModel: { m: ["t9"] },
      stepsByKey: { [cellKey("m", "t9")]: [step({})] },
      outcomeByKey: {
        [cellKey("m", "t9")]: { kind: "error", message: "CUDA error: out of memory", oom: true },
      },
    });
    render(<EvalRunPanel />);
    expect(screen.getByText("Out of memory")).toBeInTheDocument();
    expect(screen.getByTestId("eval-oom-answer").textContent).toContain("t9");
    // Dims mocked to null → the panel must refuse to invent a ceiling.
    expect(screen.getByTestId("eval-oom-answer").textContent).toContain("No safe-context suggestion");
  });
});

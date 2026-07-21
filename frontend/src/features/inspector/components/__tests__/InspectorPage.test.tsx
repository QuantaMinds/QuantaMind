import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("../../../../shared/ipc/system/vram", () => ({ loadedModels: vi.fn().mockResolvedValue([]) }));
vi.mock("../../../../shared/ipc/system/inspect", () => ({
  inspectModel: vi.fn().mockResolvedValue({ dims: null }),
  estimateKvCacheBytes: vi.fn().mockResolvedValue(0),
  contextCeilings: vi.fn().mockResolvedValue({ f16: null, q8: null, q4: null }),
}));
vi.mock("../../../../shared/ipc/compare/hardware", () => ({
  getHardwareSnapshot: vi.fn().mockResolvedValue(null),
}));

import { InspectorPage } from "../InspectorPage";
import { useBatchStore, cellKey } from "../../../eval/state/batchStore";
import type { TrajectoryStep } from "../../../../shared/ipc/eval/batch";

const step = (over: Partial<TrajectoryStep>): TrajectoryStep => ({
  run_index: 0,
  step_index: 0,
  raw_output: "{}",
  injection: null,
  kind: "tool_call",
  ...over,
});

beforeEach(() => useBatchStore.getState().reset());

/// The Latency page is the TEST-RUN cost page — workspace per-token timing moved wholly
/// under the Analysis tab (LatencyTimelines renders there with showExport).
describe("InspectorPage", () => {
  it("shows the Test-run hint when no batch has streamed, and points to Analysis for workspace timing", () => {
    render(<InspectorPage />);
    const empty = screen.getByTestId("eval-run-empty");
    expect(empty).toBeInTheDocument();
    expect(empty.textContent).toContain("Analysis");
    // The old workspace source is gone from this page.
    expect(screen.queryByTestId("latency-source-toggle")).toBeNull();
    expect(screen.queryByTestId("latency-timelines")).toBeNull();
  });

  it("renders the per-task run breakdown when a batch has streamed", () => {
    useBatchStore.setState({
      collectionId: "easy-coding",
      tasksByModel: { m: ["t1"] },
      stepsByKey: { [cellKey("m", "t1")]: [step({ prefill_ms: 10, eval_ms: 20, output_tokens: 5, prefill_tokens: 30 })] },
      outcomeByKey: {},
    });
    render(<InspectorPage />);
    expect(screen.getByTestId("eval-run-panel")).toBeInTheDocument();
    expect(screen.getByTestId("eval-task-card-t1")).toBeInTheDocument();
    expect(screen.getByTestId("eval-memory-panel")).toBeInTheDocument();
  });
});

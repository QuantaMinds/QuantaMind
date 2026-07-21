import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("../../../../shared/ipc/system/vram", async (orig) => ({
  ...(await orig<typeof import("../../../../shared/ipc/system/vram")>()),
  loadedModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../shared/ipc/workspace/history", () => ({ historyList: vi.fn().mockResolvedValue([]) }));
vi.mock("../../../../shared/ipc/compare/hardware", () => ({ getHardwareSnapshot: vi.fn().mockResolvedValue(null) }));

import { LatencyTimelines } from "../timeline/LatencyTimelines";
import { useCompareStore } from "../../../compare/state/compareStore";
import type { CompareRow } from "../../../compare/state/compareRow";
import type { TokenTiming } from "../../../../shared/ipc/events/events";

const tl = (n: number): TokenTiming[] =>
  Array.from({ length: n }, (_, i) => ({ text: `t${i}`, t_ms: i * 10, n: i + 1 }));

const doneRow = (model: string, n: number): CompareRow => ({
  model, modelId: null, status: "done", output: "x",
  metrics: { ttft_ms: 10, tokens_per_sec: 50, token_count: n, timeline: tl(n) },
  error: null, startedAt: null, endedAt: null,
});

beforeEach(() => useCompareStore.setState({ rows: [] }));

describe("LatencyTimelines", () => {
  it("renders nothing when no run has a timeline", () => {
    useCompareStore.setState({ rows: [{ ...doneRow("a", 0), metrics: { ttft_ms: 1, tokens_per_sec: 1, token_count: 0, timeline: [] } }] });
    const { container } = render(<LatencyTimelines active={false} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders one timeline panel per charted model, with the legend", () => {
    useCompareStore.setState({ rows: [doneRow("a", 3), doneRow("b", 5)] });
    render(<LatencyTimelines active={false} />);
    expect(screen.getByTestId("latency-timelines")).toBeInTheDocument();
    expect(screen.getByTestId("model-timeline-a")).toBeInTheDocument();
    expect(screen.getByTestId("model-timeline-b")).toBeInTheDocument();
    expect(screen.getAllByTestId("token-timeline")).toHaveLength(2);
    expect(screen.getByText("Prompt prefill")).toBeInTheDocument();
  });

  it("hides the export button unless asked", () => {
    useCompareStore.setState({ rows: [doneRow("a", 3)] });
    const { rerender } = render(<LatencyTimelines active={false} />);
    expect(screen.queryByTestId("export-report")).toBeNull();
    rerender(<LatencyTimelines active={false} showExport />);
    expect(screen.getByTestId("export-report")).toBeInTheDocument();
  });
});

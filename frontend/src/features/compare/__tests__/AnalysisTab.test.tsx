import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
// The latency panels rendered below the answers pull VRAM / history / hardware
// over IPC — stub them so the panel mounts with degraded (empty) data.
vi.mock("../../../shared/ipc/system/vram", async (orig) => ({
  ...(await orig<typeof import("../../../shared/ipc/system/vram")>()),
  loadedModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../shared/ipc/workspace/history", () => ({ historyList: vi.fn().mockResolvedValue([]) }));
vi.mock("../../../shared/ipc/compare/hardware", () => ({ getHardwareSnapshot: vi.fn().mockResolvedValue(null) }));

import { AnalysisTab } from "../components/AnalysisTab";
import { useCompareStore } from "../state/compareStore";
import type { TokenTiming } from "../../../shared/ipc/events/events";

const tl = (n: number): TokenTiming[] =>
  Array.from({ length: n }, (_, i) => ({ text: `t${i}`, t_ms: i * 10, n: i + 1 }));

const doneRow = (model: string, timeline: TokenTiming[] = []) => ({
  model, modelId: "u", status: "done" as const, output: `out ${model}`,
  metrics: { ttft_ms: 10, tokens_per_sec: 30, token_count: 3, timeline },
  error: null, startedAt: "s", endedAt: "e",
});

beforeEach(() => useCompareStore.getState().reset());

describe("AnalysisTab", () => {
  it("shows an empty-state until a run has produced rows", () => {
    render(<AnalysisTab />);
    expect(screen.getByTestId("analysis-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("metrics-chart")).toBeNull();
  });

  it("renders charts + diff once two models have finished", () => {
    useCompareStore.setState({ rows: [doneRow("a"), doneRow("b")] });
    render(<AnalysisTab />);
    expect(screen.queryByTestId("analysis-empty")).toBeNull();
    // Responses on top…
    expect(screen.getByTestId("compare-output-a")).toHaveTextContent("out a");
    expect(screen.getByTestId("compare-output-b")).toHaveTextContent("out b");
    // …analysis below.
    expect(screen.getByTestId("metrics-chart")).toBeInTheDocument();
    expect(screen.getByTestId("compare-diff")).toBeInTheDocument();
  });

  it("shows the full per-model latency metrics below the live answer once a run has a timeline", () => {
    useCompareStore.setState({ rows: [doneRow("a", tl(4))] });
    render(<AnalysisTab />);
    // The same per-token timing panel the Latency tab renders, now under the answer.
    expect(screen.getByTestId("latency-timelines")).toBeInTheDocument();
    expect(screen.getByTestId("model-timeline-a")).toBeInTheDocument();
    expect(screen.getByTestId("token-timeline")).toBeInTheDocument();
  });

  it("omits the latency panel when no run carries a timeline", () => {
    useCompareStore.setState({ rows: [doneRow("a")] });
    render(<AnalysisTab />);
    expect(screen.queryByTestId("latency-timelines")).toBeNull();
  });
});

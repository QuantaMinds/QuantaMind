import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { HistoryTimeline } from "../../components/matrix/HistoryTimeline";
import type { RunSummary } from "../../../../shared/ipc/eval/matrix";

// Mirror the component's plot geometry so we can assert exact point placement.
const W = 620, ML = 48, MR = 16, MT = 16;
const PW = W - ML - MR; // 556 → left edge = ML (48), right edge = ML+PW (604), center = 326

const run = (model: string, composite: number | null, extra: Partial<RunSummary> = {}): RunSummary => ({
  ts: "2026-06-01T00:00:00Z", model, backend: "ollama",
  parse_rate: null, tool_selection_acc: null, arg_acc: null, abstain_acc: null,
  composite, n: 5, ...extra,
});

const coords = (line: Element, axis: 0 | 1) =>
  line.getAttribute("points")!.trim().split(/\s+/).map((pt) => Number(pt.split(",")[axis]));

const hist = [
  { ts: "t1", model: "m1", backend: "ollama", parse_rate: 1, tool_selection_acc: 1, arg_acc: 1, abstain_acc: null, composite: 0.8, n: 3 },
  { ts: "t2", model: "m1", backend: "ollama", parse_rate: 1, tool_selection_acc: 1, arg_acc: 1, abstain_acc: null, composite: 0.6, n: 3 },
] as never;

describe("HistoryTimeline", () => {
  it("shows an empty state with no history", () => {
    render(<HistoryTimeline history={[]} />);
    expect(screen.getByTestId("eval-history-empty")).toBeTruthy();
  });

  it("renders one polyline series per model", () => {
    const { container } = render(<HistoryTimeline history={hist} />);
    expect(screen.getByTestId("eval-history-timeline")).toBeTruthy();
    expect(container.querySelectorAll("polyline")).toHaveLength(1);
  });

  it("spans each model across its OWN run ordinal — fewer-run models aren't left-packed", () => {
    const { container } = render(
      <HistoryTimeline
        history={[
          run("A", 0.9), run("A", 0.5), // 2 runs
          run("B", 0.8), run("B", 0.7), run("B", 0.6), // 3 runs
        ]}
      />,
    );
    const lines = Array.from(container.querySelectorAll("polyline"));
    expect(lines).toHaveLength(2);
    // Both series start at the left edge (run #1) and end at the right edge (latest run),
    // regardless of run-count — the old shared-maxLen axis packed the 2-run model short.
    for (const line of lines) {
      const xs = coords(line, 0);
      expect(xs[0]).toBeCloseTo(ML);
      expect(xs[xs.length - 1]).toBeCloseTo(ML + PW);
    }
  });

  it("places a single-run series at the left edge (run #1), not dead-center", () => {
    const { container } = render(<HistoryTimeline history={[run("A", 0.9)]} />);
    const xs = coords(container.querySelector("polyline")!, 0);
    expect(xs[0]).toBeCloseTo(ML); // not the center (326)
    expect(xs[0]).not.toBeCloseTo(ML + PW / 2);
  });

  it("clamps an out-of-range score to the axis edge instead of rendering off-canvas", () => {
    const { container } = render(<HistoryTimeline history={[run("A", 1.5), run("A", 0.9)]} />);
    const ys = coords(container.querySelector("polyline")!, 1);
    expect(ys[0]).toBeCloseTo(MT); // 1.5 clamps to the top edge, never a negative y
  });

  it("notes runs dropped for having no composite or Pass^k, rather than silently shrinking", () => {
    render(<HistoryTimeline history={[run("A", 0.9), run("A", null, { pass_k: null })]} />);
    expect(screen.getByTestId("eval-history-dropped")).toHaveTextContent("1 run had no composite or Pass^k");
  });
});

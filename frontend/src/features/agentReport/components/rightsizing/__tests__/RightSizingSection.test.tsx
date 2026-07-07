import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { RightSizingSection } from "../RightSizingSection";
import type { RightSizingGroup, RightSizingPick } from "../../../../../shared/ipc/eval/readiness";

const pick = (model: string, weights: number, status: RightSizingPick["status"], q = "Q4_K_M"): RightSizingPick => ({
  model,
  quantization: q,
  weights_bytes: weights,
  total_bytes: null,
  pass_k: 0.85,
  status,
});

const group = (over: Partial<RightSizingGroup> = {}): RightSizingGroup => ({
  group: "qwen 9B",
  baseline: pick("qwen-q8", 9e9, "ready", "Q8_0"),
  pick: pick("qwen-q4", 5e9, "ready"),
  size_reduction_pct: 44.4,
  memory_reduction_pct: 44.4,
  quality_delta_pp: -5.0,
  pick_is_conditional: false,
  rationale: "smallest Ready variant on this hardware",
  ...over,
});

describe("RightSizingSection", () => {
  it("renders percent reductions and the Pass^k delta as pp", () => {
    render(<RightSizingSection groups={[group()]} hint={null} />);
    const card = screen.getByTestId("rs-group-qwen 9B");
    expect(card).toHaveTextContent("−44% size");
    expect(card).toHaveTextContent("−44% memory");
    expect(card).toHaveTextContent("Pass^k -5.0 pp");
  });

  it("omits the memory chip when memory_reduction_pct is unmeasured", () => {
    render(<RightSizingSection groups={[group({ memory_reduction_pct: null })]} hint={null} />);
    expect(screen.getByTestId("rs-group-qwen 9B")).not.toHaveTextContent("% memory");
  });

  it("shows the amber advisory when the pick is Conditional", () => {
    render(
      <RightSizingSection
        groups={[group({ pick: pick("qwen-q4", 5e9, "conditional"), pick_is_conditional: true })]}
        hint={null}
      />,
    );
    expect(screen.getByTestId("rs-advisory-qwen 9B")).toHaveTextContent("Conditional");
  });

  it("shows 'Pass^k not measured' rather than a fabricated 0", () => {
    render(<RightSizingSection groups={[group({ quality_delta_pp: null })]} hint={null} />);
    expect(screen.getByTestId("rs-group-qwen 9B")).toHaveTextContent("Pass^k not measured");
  });

  it("renders the backend hint when there are no groups", () => {
    render(<RightSizingSection groups={[]} hint="Assess ≥2 quants of the same family to compare right-sizing." />);
    expect(screen.getByTestId("rs-hint")).toHaveTextContent("Assess ≥2 quants");
  });

  it("renders nothing when there are neither groups nor a hint", () => {
    const { container } = render(<RightSizingSection groups={[]} hint={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("collapses to an 'already smallest' note when pick == baseline", () => {
    const same = pick("qwen-q8", 9e9, "ready", "Q8_0");
    render(<RightSizingSection groups={[group({ baseline: same, pick: same, size_reduction_pct: 0 })]} hint={null} />);
    expect(screen.getByTestId("rs-group-qwen 9B")).toHaveTextContent("already the smallest usable variant");
  });

  it("contains no dollar or currency text anywhere", () => {
    render(<RightSizingSection groups={[group()]} hint={null} />);
    expect(screen.getByTestId("right-sizing").textContent).not.toMatch(/[$€£]|dollar|\/hr|per hour|cost/i);
  });
});

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { BoundaryPanel } from "../components/BoundaryPanel";
import { BoundaryReportSchema } from "../../../shared/ipc/eval/batch";
import type { BatchReport, BoundaryReport } from "../../../shared/ipc/eval/batch";

/// A real-shaped backend BoundaryReport payload (snake_case, as serde emits it) — used to
/// prove the Zod mirror parses what the backend sends, then to render the panel.
const RAW_BOUNDARY = {
  attack_probes: 6,
  resisted: 3,
  resistance: 0.5,
  benign_probes: 3,
  over_refusals: 3,
  over_refusal_rate: 1.0,
  by_attack: [
    { attack: "file_injection", resisted: 0, total: 3 },
    { attack: "system_injection", resisted: 3, total: 3 },
  ],
  attribution: { model_followed: 3, guard_truncated: 0, unattributed: 0 },
  native_fc: false,
  gate: { status: "fail", resistance: 0.5, over_refusal: 1.0 },
  caveat: "Boundary resistance measures a fixed, known-injection set — NOT a guarantee against adaptive attacks.",
};

const reportWith = (b: BoundaryReport): BatchReport =>
  ({
    collection_id: "boundary-healthcare",
    columns: [
      {
        model: "llama-3.2-1b",
        backend: "ollama",
        toolcall: null,
        agentic: {
          tasks_passed: 0,
          tasks_total: 3,
          passes: 0,
          total_runs: 9,
          avg_steps: null,
          avg_output_tokens_success: null,
          schema_resilience: null,
          top_error: "forbidden_call",
          failures: { infinite_loop_hits: 0, hallucinated_completions: 0, malformed_json_calls: 0, schema_unrecovered_calls: 0 },
          boundary: b,
        },
        agentic_native_fc: null,
        error: null,
      },
    ],
  }) as unknown as BatchReport;

describe("BoundaryPanel", () => {
  it("Zod parses a real backend BoundaryReport payload", () => {
    const parsed = BoundaryReportSchema.parse(RAW_BOUNDARY);
    expect(parsed.resistance).toBe(0.5);
    expect(parsed.gate.status).toBe("fail");
    expect(parsed.attribution.model_followed).toBe(3);
  });

  it("renders resistance, over-refusal, gate, attribution and the caveat", () => {
    const b = BoundaryReportSchema.parse(RAW_BOUNDARY);
    render(<BoundaryPanel report={reportWith(b)} />);
    expect(screen.getByTestId("boundary-panel")).toBeTruthy();
    expect(screen.getByTestId("boundary-gate-fail")).toBeTruthy();
    // Both arms shown, each with its own denominator sub-text (unique).
    expect(screen.getByText("50%")).toBeTruthy(); // resistance metric value (unique)
    expect(screen.getByText("3/6 attack runs resisted")).toBeTruthy();
    expect(screen.getByText("3/3 benign runs refused")).toBeTruthy();
    // Attribution split names the model verdict.
    expect(screen.getByText(/Model followed injection: 3/)).toBeTruthy();
    // Per-vector rows present.
    expect(screen.getByTestId("boundary-vector-file_injection")).toBeTruthy();
    // The static-set caveat is rendered (G2), not omitted.
    expect(screen.getByText(/fixed, known-injection set/)).toBeTruthy();
  });

  it("renders nothing when no column carries a boundary (capability-only run)", () => {
    const plain = { collection_id: "easy-coding", columns: [] } as unknown as BatchReport;
    const { container } = render(<BoundaryPanel report={plain} />);
    expect(screen.queryByTestId("boundary-panel")).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });

  it("shows an honest em-dash and Inconclusive gate when the benign arm didn't run", () => {
    const b = BoundaryReportSchema.parse({
      ...RAW_BOUNDARY,
      benign_probes: 0,
      over_refusals: 0,
      over_refusal_rate: null,
      gate: { status: "inconclusive" },
    });
    render(<BoundaryPanel report={reportWith(b)} />);
    expect(screen.getByTestId("boundary-gate-inconclusive")).toBeTruthy();
    expect(screen.getByText("—")).toBeTruthy(); // over-refusal has no value → em-dash, never a fabricated 0
  });
});

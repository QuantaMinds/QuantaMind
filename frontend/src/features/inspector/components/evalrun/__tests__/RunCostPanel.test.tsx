import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { RunCostPanel } from "../RunCostPanel";
import type { RunCostSummary } from "../../../../../shared/ipc/eval/batch";

const priced: RunCostSummary = {
  basis: "gpu_seconds",
  basis_note: "serial wall-clock at $0.98/hr, utilization=1.00. Upper bound: production batching amortizes fixed cost across concurrent requests.",
  cost_per_attempt_usd: 0.021,
  cost_per_task_usd: 0.11,
  cost_per_success_usd: 0.136,
  run_total_usd: 2.31,
  excluded_truncated: 3,
  cost_measured: true,
};

describe("RunCostPanel", () => {
  it("shows per-task and per-success side by side — the gap IS the point", () => {
    render(<RunCostPanel costs={priced} />);
    expect(screen.getByTestId("cost-per-task")).toHaveTextContent("$0.110");
    expect(screen.getByTestId("cost-per-success")).toHaveTextContent("$0.136");
    expect(screen.getByTestId("cost-run-total")).toHaveTextContent("$2.310");
  });

  /// The whole contamination guard, on screen: excluded runs are named, never
  /// quietly dropped from a figure that then looks cheap.
  it("states the excluded runs and why they were left out", () => {
    render(<RunCostPanel costs={priced} />);
    const note = screen.getByTestId("cost-excluded");
    expect(note).toHaveTextContent("3 attempts excluded");
    expect(note).toHaveTextContent("cut at the output cap");
  });

  it("always shows the basis note — the upper-bound caveat is not optional", () => {
    render(<RunCostPanel costs={priced} />);
    expect(screen.getByTestId("cost-basis-note")).toHaveTextContent(/upper bound/i);
  });

  /// No price must never render as $0.00 — that reads as "this run was free"
  /// rather than "we have no price".
  it("renders n/a (never $0.00) when there is no price basis", () => {
    const unpriced: RunCostSummary = {
      basis: "gpu_seconds",
      basis_note: "no price basis: set costs.gpu_hourly_usd to price this run",
      cost_per_attempt_usd: null,
      cost_per_task_usd: null,
      cost_per_success_usd: null,
      run_total_usd: null,
      excluded_truncated: 0,
      cost_measured: false,
    };
    render(<RunCostPanel costs={unpriced} />);
    expect(screen.getByTestId("cost-per-task")).toHaveTextContent("n/a");
    expect(screen.getByTestId("cost-per-success")).toHaveTextContent("n/a");
    expect(screen.getByTestId("run-cost-no-basis")).toBeInTheDocument();
    expect(screen.queryByText("$0.000")).not.toBeInTheDocument();
  });

  /// A run that succeeded at nothing has a bill but no cost-per-success — that
  /// cell must read n/a, not 0 and not infinity.
  it("shows n/a for cost-per-success when the run cleared no tasks", () => {
    render(<RunCostPanel costs={{ ...priced, cost_per_success_usd: null }} />);
    expect(screen.getByTestId("cost-per-success")).toHaveTextContent("n/a");
    expect(screen.getByTestId("cost-run-total")).toHaveTextContent("$2.310"); // the bill stands
  });

  it("says nothing about exclusions when there were none", () => {
    render(<RunCostPanel costs={{ ...priced, excluded_truncated: 0 }} />);
    expect(screen.queryByTestId("cost-excluded")).not.toBeInTheDocument();
  });
});

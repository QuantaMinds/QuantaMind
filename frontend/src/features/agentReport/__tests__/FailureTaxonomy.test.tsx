import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { FailureTaxonomy } from "../components/FailureTaxonomy";
import type { Tier, TierStat } from "../../../shared/ipc/eval/readiness";
import type { FailureTracker } from "../../../shared/ipc/eval/batch";

const stat = (tier: Tier, failures: Partial<FailureTracker>): TierStat => ({
  tier,
  tasks_passed: 0,
  tasks_total: 1,
  avg_steps: null,
  failures: {
    infinite_loop_hits: 0,
    hallucinated_completions: 0,
    malformed_json_calls: 0,
    schema_unrecovered_calls: 0,
    unknown_tool_calls: 0,
    forbidden_calls: 0,
    turn_timeouts: 0,
    ...failures,
  },
});

describe("FailureTaxonomy", () => {
  it("renders nothing until a tier is selected (failures are tier-gated)", () => {
    const { container } = render(<FailureTaxonomy tier={null} />);
    expect(screen.queryByTestId("failure-taxonomy")).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });

  it("shows ONLY the selected tier's failures, sorted desc, as share-of-events %", () => {
    render(
      <FailureTaxonomy
        tier={stat("hard", { unknown_tool_calls: 30, forbidden_calls: 20, infinite_loop_hits: 10 })}
      />,
    );
    // Heading names the single selected tier (not a cross-tier sum).
    expect(screen.getByTestId("failure-taxonomy")).toHaveTextContent("— Hard");
    // Percentages are over REAL failures only (30 events): the backend counts
    // unknown_tool_calls on passing runs too and excludes it from top(), so it must
    // never deflate the failure shares: ForbiddenCall 20/30=67%, InfiniteLoop 10/30=33%.
    expect(screen.queryByTestId("failure-row-unknown_tool_calls")).toBeNull();
    expect(screen.getByTestId("failure-row-forbidden_calls")).toHaveTextContent("67%");
    expect(screen.getByTestId("failure-row-infinite_loop_hits")).toHaveTextContent("33%");
    expect(screen.getByTestId("failure-taxonomy")).toHaveTextContent("tracked failure events");
    // ...but the diagnostic count is still visible, framed as a non-failure.
    expect(screen.getByTestId("failure-diagnostic-unknown-tool")).toHaveTextContent("30");
    expect(screen.getByTestId("failure-diagnostic-unknown-tool")).toHaveTextContent("not a failure");
    // A zero mode is omitted entirely.
    expect(screen.queryByTestId("failure-row-turn_timeouts")).toBeNull();
  });

  it("a selected tier with no failures shows the empty state for that tier", () => {
    render(<FailureTaxonomy tier={stat("medium", {})} />);
    expect(screen.getByTestId("failure-taxonomy-empty")).toBeInTheDocument();
    expect(screen.getByTestId("failure-taxonomy")).toHaveTextContent("— Medium");
  });

  it("unknown-tool-only events show the empty state plus the diagnostic, never a 100% failure bar", () => {
    // Every run passed; the model merely poked decoys. That is NOT a failure distribution.
    render(<FailureTaxonomy tier={stat("easy", { unknown_tool_calls: 4 })} />);
    expect(screen.getByTestId("failure-taxonomy-empty")).toBeInTheDocument();
    expect(screen.getByTestId("failure-diagnostic-unknown-tool")).toHaveTextContent("4");
  });
});

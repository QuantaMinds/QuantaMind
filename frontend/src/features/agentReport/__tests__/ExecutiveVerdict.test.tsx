import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ExecutiveVerdict } from "../components/ExecutiveVerdict";
import type { ModelVerdict, Tier, TierStat } from "../../../shared/ipc/eval/readiness";
import type { HardwareTier } from "../../../shared/ipc/compare/hardware";

const ft = () => ({
  infinite_loop_hits: 0,
  hallucinated_completions: 0,
  malformed_json_calls: 0,
  schema_unrecovered_calls: 0,
  unknown_tool_calls: 0,
  forbidden_calls: 0,
  turn_timeouts: 0,
});
const stat = (tier: Tier, passed: number, total: number): TierStat => ({
  tier,
  tasks_passed: passed,
  tasks_total: total,
  avg_steps: 2,
  failures: ft(),
});
const mkVerdict = (by_tier: TierStat[], cleared_tier: Tier | null): ModelVerdict =>
  ({
    model: "m",
    backend: "ollama",
    verdict: { status: "conditional", blocking: [], conditions: [], path: "prompt_based", required_tier: "hard", cleared_tier },
    by_tier,
  }) as unknown as ModelVerdict;

const workstation: HardwareTier = { total_memory_bytes: 64 * 1024 ** 3, class: "Workstation", recommended_tier: "hard" };

const renderEV = (by_tier: TierStat[], cleared: Tier | null, hw: HardwareTier | null = null) =>
  render(<ExecutiveVerdict verdict={mkVerdict(by_tier, cleared)} hardwareTier={hw} minPassK={0.8} />);

describe("ExecutiveVerdict — run-tier headline + curve-aware Lens 1", () => {
  it("monotonic-full → READY, 'clears every tier tested'", () => {
    renderEV([stat("easy", 1, 1), stat("medium", 1, 1)], "medium");
    expect(screen.getByTestId("exec-verdict-status")).toHaveTextContent("READY");
    expect(screen.getByTestId("exec-verdict-status")).not.toHaveTextContent("NOT READY");
    expect(screen.getByTestId("exec-verdict-lens1")).toHaveTextContent("Clears every tier tested, through Medium.");
  });

  it("monotonic-partial → CONDITIONAL, 'clears through {prefix}; falls off'", () => {
    renderEV([stat("easy", 1, 1), stat("hard", 0, 1)], "easy");
    expect(screen.getByTestId("exec-verdict-status")).toHaveTextContent("CONDITIONAL");
    expect(screen.getByTestId("exec-verdict-lens1")).toHaveTextContent(
      "Clears through Easy; falls off at Hard — the most demanding tier tested.",
    );
  });

  it("non-monotonic → CONDITIONAL, 'cleared X but missed a lower tier' (distinct from nothing-cleared)", () => {
    renderEV([stat("easy", 0, 1), stat("hard", 1, 1)], "hard");
    expect(screen.getByTestId("exec-verdict-status")).toHaveTextContent("CONDITIONAL");
    expect(screen.getByTestId("exec-verdict-lens1")).toHaveTextContent(
      "Cleared Hard but missed a lower tier — inconsistent; treat as not production-ready at Hard.",
    );
  });

  it("nothing-cleared → NOT READY, 'does not clear the easiest tier' (must differ from non-monotonic)", () => {
    renderEV([stat("easy", 0, 1)], null);
    expect(screen.getByTestId("exec-verdict-status")).toHaveTextContent("NOT READY");
    expect(screen.getByTestId("exec-verdict-lens1")).toHaveTextContent("Does not clear Easy, the easiest tier tested.");
    // The two clearsThrough===null cases render DIFFERENT strings.
    expect(screen.getByTestId("exec-verdict-lens1")).not.toHaveTextContent("missed a lower tier");
  });

  it("below-recommendation advisory fires WITHOUT downgrading status", () => {
    renderEV([stat("easy", 1, 1)], "easy", workstation);
    // Ran Easy on Workstation HW (recommends Hard): advisory present…
    expect(screen.getByTestId("exec-verdict-advisory")).toHaveTextContent("run a harder tier");
    // …but the run-tier status is still READY (the chosen tier is never force-failed).
    expect(screen.getByTestId("exec-verdict-status")).toHaveTextContent("READY");
    expect(screen.getByTestId("exec-verdict-status")).not.toHaveTextContent("NOT READY");
  });

  it("empty by_tier → no tier framing", () => {
    renderEV([], null);
    expect(screen.getByTestId("exec-verdict-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("exec-verdict-status")).toBeNull();
  });

  it("VISIBLY renders the hardware lens (was hidden test-only)", () => {
    // Even when the run is NOT below recommendation (Easy tested is all we ran here
    // but hw present): the measured class + its recommended tier must be readable
    // by a user, not just by vitest via the hidden shim.
    renderEV([stat("easy", 1, 1), stat("hard", 1, 1)], "hard", workstation);
    const visible = screen.getByTestId("exec-verdict-hw-lens-visible");
    expect(visible).not.toHaveClass("hidden");
    expect(visible).toHaveTextContent("HW: Workstation (64GB RAM) · recommends HARD.");
    // And it matches the hidden shim's text exactly (one source of truth).
    expect(screen.getByTestId("exec-verdict-hw-lens").textContent).toBe(visible.textContent);
  });
});

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { CacheBadge } from "../TraceDebugger";
import type { TrajectoryStep } from "../../../../shared/ipc/eval/batch";

// cache_n = reused; prefill_tokens = recomputed (prompt_n); total = cache_n + prefill_tokens.
const step = (over: Partial<TrajectoryStep>): TrajectoryStep => ({
  run_index: 0,
  step_index: 1,
  raw_output: "",
  injection: null,
  kind: "tool_call",
  ...over,
});

describe("CacheBadge (per-turn prefix-cache state)", () => {
  it("healthy non-first turn → GREEN reused/recomputed (normal new-token recompute is NOT a bust)", () => {
    // Warm: 39 reused + 1 recomputed = 40 total, ratio 0.975 → green.
    render(<CacheBadge s={step({ step_index: 1, cache_n: 39, prefill_tokens: 1 })} />);
    expect(screen.getByTestId("cache-badge-hit")).toHaveTextContent("39 reused / 1 recomputed");
    expect(screen.queryByTestId("cache-badge-bust")).toBeNull();
  });

  it("non-first turn whose prefix collapsed → AMBER cache bust with the prefill ms", () => {
    // Bust: nothing reused, 250 recomputed (ratio 0) on a turn that had a prefix.
    render(<CacheBadge s={step({ step_index: 2, cache_n: 0, prefill_tokens: 250, prefill_ms: 300 })} />);
    const b = screen.getByTestId("cache-badge-bust");
    expect(b).toHaveTextContent("250 re-prefilled");
    expect(b).toHaveTextContent("+300ms");
  });

  it("first turn (step_index 0) → NEUTRAL with the total prompt, never amber (cold prefill is expected)", () => {
    render(<CacheBadge s={step({ step_index: 0, cache_n: 0, prefill_tokens: 250 })} />);
    expect(screen.getByTestId("cache-badge-first")).toHaveTextContent("prefill · 250 tok");
    expect(screen.queryByTestId("cache-badge-bust")).toBeNull();
  });

  it("a remote backend (cache_n null) → NO badge at all (absence-of-feature, not a false zero)", () => {
    const { container } = render(<CacheBadge s={step({ step_index: 1, cache_n: null, prefill_tokens: null })} />);
    expect(container).toBeEmptyDOMElement();
  });
});

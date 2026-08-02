import { describe, it, expect } from "vitest";
import { CLIFF_CTX_HEADROOM, cliffHeadroom, cliffThinkTokens, cliffTierForDepth, usableCliffTokens } from "../cliff";

/// The probe runs at `maxTokens + CLIFF_CTX_HEADROOM` (readiness_cmd.rs::run_context_cliff).
/// Offering the model's FULL window as Max Tokens therefore asked for MORE context than the
/// model has — for every model, by exactly the headroom. the server answers that by silently
/// clamping `num_ctx` to the trained window and truncating the prompt: the needle is deleted,
/// the rung fails, and `prompt_eval_count` saturates at the window so the reported depth is
/// fabricated. llama.cpp instead refuses with "raise the context window" — impossible, since
/// the model has no more. `usableCliffTokens` is the invariant that prevents both.
describe("usableCliffTokens", () => {
  it("leaves room for the headroom the backend adds, so the request fits the window", () => {
    // The regression, stated as the invariant: requested_ctx must never exceed the window.
    for (const contextLength of [32768, 131072, 262144]) {
      const requested = usableCliffTokens(contextLength) + CLIFF_CTX_HEADROOM;
      expect(requested).toBeLessThanOrEqual(contextLength);
    }
  });

  it("uses the whole window apart from the headroom — no needless loss of depth", () => {
    expect(usableCliffTokens(32768)).toBe(32768 - CLIFF_CTX_HEADROOM);
    expect(usableCliffTokens(262144)).toBe(262144 - CLIFF_CTX_HEADROOM);
  });

  it("never inverts the slider range on a tiny or misreported window", () => {
    // Below the slider's 4096 floor the subtraction would go negative / under the min.
    expect(usableCliffTokens(4096)).toBe(4096);
    expect(usableCliffTokens(1024)).toBe(4096);
    expect(usableCliffTokens(0)).toBe(4096);
  });
});

/// The thinking budget scales with DEPTH through the same canonical per-tier table the
/// Tests page shows (mirrors backend `cliff::budget` — tier_for_depth + think_tokens).
describe("cliff thinking budget (depth-banded)", () => {
  it("maps depths to the tier bands at exact boundaries", () => {
    expect(cliffTierForDepth(0)).toBe("easy");
    expect(cliffTierForDepth(4096)).toBe("easy");
    expect(cliffTierForDepth(4097)).toBe("medium");
    expect(cliffTierForDepth(8192)).toBe("medium");
    expect(cliffTierForDepth(8193)).toBe("hard");
    expect(cliffTierForDepth(16384)).toBe("hard");
    expect(cliffTierForDepth(16385)).toBe("extreme");
  });

  it("adds no scratchpad for a non-thinking run — headroom stays the base", () => {
    for (const depth of [0, 9216, 65536]) {
      expect(cliffHeadroom(depth, false, "deep")).toBe(CLIFF_CTX_HEADROOM);
      expect(cliffThinkTokens(depth, false, "deep")).toBe(0);
    }
  });

  it("grows the scratchpad with depth and preset (never shrinks as rungs deepen)", () => {
    for (const preset of ["lean", "standard", "deep"] as const) {
      const budgets = [4096, 8192, 16384, 32768].map((d) => cliffThinkTokens(d, true, preset));
      for (let i = 1; i < budgets.length; i++) expect(budgets[i]).toBeGreaterThan(budgets[i - 1]);
    }
    // Standard at a 9216-token rung sits in the hard band: +10240 (the Tests page table).
    expect(cliffThinkTokens(9216, true, "standard")).toBe(10240);
  });

  it("keeps the thinking slider cap self-consistent: depth + its own headroom fits the window", () => {
    for (const preset of ["lean", "standard", "deep"] as const) {
      for (const window of [8192, 16384, 21000, 32768, 131072, 262144]) {
        const usable = usableCliffTokens(window, true, preset);
        // The invariant the backend gate enforces — requested_ctx never exceeds the window
        // (floored windows below the 4096 slider minimum are the gate's job, not the slider's).
        if (usable + cliffHeadroom(usable, true, preset) > window) {
          expect(usable).toBe(4096); // only the floor may "exceed" — a tiny window can't invert the slider
        }
      }
    }
  });

  it("reserves more of the window when thinking than not (never a free scratchpad)", () => {
    expect(usableCliffTokens(32768, true, "deep")).toBeLessThan(usableCliffTokens(32768));
  });
});

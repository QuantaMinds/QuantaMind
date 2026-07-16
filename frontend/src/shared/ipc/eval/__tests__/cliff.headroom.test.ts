import { describe, it, expect } from "vitest";
import { CLIFF_CTX_HEADROOM, usableCliffTokens } from "../cliff";

/// The probe runs at `maxTokens + CLIFF_CTX_HEADROOM` (readiness_cmd.rs::run_context_cliff).
/// Offering the model's FULL window as Max Tokens therefore asked for MORE context than the
/// model has — for every model, by exactly the headroom. Ollama answers that by silently
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

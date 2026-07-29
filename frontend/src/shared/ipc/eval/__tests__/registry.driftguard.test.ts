import { describe, it, expect } from "vitest";
import { AgenticSpecSchema } from "../registry";
// Vite `?raw`: the Rust source as a string, so the guard reads the REAL struct —
// not a hand-maintained mirror list that would itself drift.
import specRs from "../../../../../../backend/src/inference/eval/agentic/spec.rs?raw";

/// The frontend hands parsed tasks straight back to `run_batch_eval`, and `z.object()`
/// STRIPS unlisted keys — so every `pub` field of the Rust `AgenticSpec` must appear in
/// `AgenticSpecSchema` or it is silently dropped on the GUI round-trip while the CLI and
/// backend tests stay green. That exact gap shipped three stripped fields
/// (`field_projections` — leaky getters; `payload_noise`; `mcp`). This guard turns the
/// next new Rust field into a red test instead of a silent behavior fork.
describe("AgenticSpecSchema mirrors the Rust AgenticSpec (round-trip strip guard)", () => {
  it("lists every pub field of the Rust struct", () => {
    const structBody = specRs.match(/pub struct AgenticSpec \{([\s\S]*?)\n\}/)?.[1];
    expect(structBody, "AgenticSpec struct not found in spec.rs — update the guard's path/regex").toBeTruthy();
    const rustFields = [...structBody!.matchAll(/^\s*pub ([a-z_]+):/gm)].map((m) => m[1]);
    // Sanity: the regex actually found the struct's fields (not a stale/empty match).
    expect(rustFields.length).toBeGreaterThanOrEqual(19);
    const tsKeys = new Set(Object.keys(AgenticSpecSchema.shape));
    const missing = rustFields.filter((f) => !tsKeys.has(f));
    expect(missing, `Rust AgenticSpec field(s) missing from AgenticSpecSchema — z.object() will strip them on the GUI round-trip: ${missing.join(", ")}`).toEqual([]);
  });
});

import { describe, it, expect } from "vitest";
import { buildReadinessMarkdown } from "../markdown";
import type { ModelVerdict, ReadinessProfile } from "../../../../shared/ipc/eval/readiness";
import type { HardwareSnapshot } from "../../../../shared/ipc/compare/hardware";

const PROFILE: ReadinessProfile = {
  id: "coding", name: "Coding agent", min_pass_k: 0.8, max_avg_steps: null, max_ms_per_step: null,
  min_context_tokens: null, forbid_infinite_loop: true, forbid_hallucinated_completion: true,
  require_full_vram: false, require_native_fc: false,
};

const READY: ModelVerdict = {
  model: "llama3.1:8b", backend: "ollama",
  verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
  pass_k: 0.85, effort: 1.23, avg_steps: 3.0, quantization: "Q4_K_M",
  passes: 14, total_runs: 16, // run-level 88%, distinct from the strict pass_k
};

const UNMEASURED: ModelVerdict = {
  model: "mistral:7b", backend: "llama_cpp",
  verdict: { status: "not_ready", blocking: ["Pass^k 40% < required 80%"], conditions: ["high VRAM pressure"], path: "prompt_based" },
  pass_k: null, effort: null, avg_steps: null,
};

const HW: HardwareSnapshot = {
  total_memory_bytes: 32 * 1024 ** 3, available_memory_bytes: 18 * 1024 ** 3,
  is_apple_silicon: true, cpu: "Apple M3 Pro", estimated_bandwidth_gbps: 150,
  gpu: { unified: true, available: true, name: "Apple M3 Pro", vram_total_bytes: 32 * 1024 ** 3 },
};

const ISO = "2026-06-06T12:00:00.000Z";

describe("buildReadinessMarkdown", () => {
  it("emits a GFM table with the disclaimer, profile gates, and hardware header", () => {
    const md = buildReadinessMarkdown([READY], PROFILE, "agentic-3", ISO, HW);
    expect(md).toContain("# Local Agent Readiness — agentic-3");
    expect(md).toContain("not objective truth");
    expect(md).toContain("**Hardware**");
    expect(md).toContain("Apple M3 Pro");
    expect(md).toContain("Apple Silicon, unified");
    expect(md).toContain("**Profile gates (Coding agent):**");
    expect(md).toContain("| Model | Backend | Readiness | Pass^k | Runs | Effort | Steps |");
    expect(md).toContain("|---|---|---|---|---|---|---|");
    expect(md).toContain("| llama3.1:8b | ollama | READY | 85% | 14/16 (88%) | 1.23 | 3.0 |");
  });

  it("maps every unmeasured metric to N/A and never leaks an undefined literal", () => {
    const md = buildReadinessMarkdown([UNMEASURED], PROFILE, "agentic-3", ISO, HW);
    expect(md).toContain("| mistral:7b | llama_cpp | NOT READY | N/A | N/A | N/A | N/A |");
    expect(md).not.toContain("undefined");
  });

  it("renders per-model diagnostic reasons (✗ blocking, ! conditions, ✓ clean)", () => {
    const md = buildReadinessMarkdown([READY, UNMEASURED], PROFILE, "c", ISO, HW);
    expect(md).toContain("### llama3.1:8b — READY (Native FC)");
    expect(md).toContain("- ✓ Meets all criteria");
    expect(md).toContain("### mistral:7b — NOT READY (Prompt-Based)");
    expect(md).toContain("- ✗ Pass^k 40% < required 80%");
    expect(md).toContain("- ! high VRAM pressure");
  });

  it("works with no hardware and no verdicts without emitting undefined", () => {
    const md = buildReadinessMarkdown([], PROFILE, "empty", ISO, null);
    expect(md).not.toContain("**Hardware**");
    expect(md).toContain("_No models assessed._");
    expect(md).not.toContain("undefined");
  });
});

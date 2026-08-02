import { describe, it, expect } from "vitest";
import { buildReadinessHtml } from "../reportHtml";
import type { ModelVerdict, ReadinessProfile } from "../../../shared/ipc/eval/readiness";

const profile: ReadinessProfile = {
  id: "coding-agent",
  name: "Coding agent",
  min_pass_k: 0.8,
  max_avg_steps: null,
  max_ms_per_step: 5000,
  min_context_tokens: null,
  forbid_infinite_loop: true,
  forbid_hallucinated_completion: true,
  require_full_vram: false,
  require_native_fc: false,
};

const GIB = 1024 ** 3;

const verdicts: ModelVerdict[] = [
  {
    model: "qwen",
    backend: "llama_cpp",
    verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" },
    memory: {
      weights_bytes: 5 * GIB,
      kv_cache_bytes: 1 * GIB,
      total_bytes: 6 * GIB,
      cap_bytes: 24 * GIB,
      context_length: 8192,
      fits: true,
      pressure: false,
    },
  },
  {
    model: "phi3.5",
    backend: "llama_cpp",
    verdict: {
      status: "not_ready",
      blocking: ["pass^k 0.40 < 0.80 required", "tool <name> not in <schema>"],
      conditions: [],
      path: "prompt_based",
    },
  },
];

describe("buildReadinessHtml", () => {
  it("is a self-contained utf-8 document with the verdicts and reasons", () => {
    const html = buildReadinessHtml(verdicts, profile, "finance", "2026-06-05T00:00:00Z");
    expect(html).toContain('<meta charset="utf-8">');
    expect(html).toContain("READY");
    expect(html).toContain("NOT READY");
    expect(html).toContain("pass^k 0.40 &lt; 0.80 required"); // interpolated math, escaped
    expect(html).toContain("finance");
    expect(html).toContain("Coding agent");
    // The memory footprint line is in the export (escaped < for the cap comparison).
    expect(html).toContain("VRAM: 6.0 GB (5.0 model + 1.0 cache @ 8k ctx) &lt; 24.0 GB cap · fits");
  });

  it("escapes all interpolated text — no raw angle brackets from a reason leak through", () => {
    const html = buildReadinessHtml(verdicts, profile, "finance", "2026-06-05T00:00:00Z");
    expect(html).toContain("tool &lt;name&gt; not in &lt;schema&gt;");
    expect(html).not.toContain("tool <name> not in <schema>");
  });
});

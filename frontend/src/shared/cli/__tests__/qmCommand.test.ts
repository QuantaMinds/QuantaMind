import { describe, it, expect } from "vitest";
import { buildRunCommand, buildCliffCommand, buildReportCommand, buildPromptCommand, buildRunHistory, paramFlags, modeFrom, shellQuote } from "../qmCommand";

describe("paramFlags", () => {
  it("emits ONLY the params the user set (all 7 supported, unset omitted)", () => {
    expect(paramFlags(undefined)).toEqual([]);
    expect(paramFlags({})).toEqual([]);
    expect(paramFlags({ temperature: 0.7, top_p: 0.9, top_k: 40, max_tokens: 512, repeat_penalty: 1.1, seed: 42, num_ctx: 8192 })).toEqual([
      "--temperature 0.7", "--top-p 0.9", "--top-k 40", "--num-predict 512", "--repeat-penalty 1.1", "--seed 42", "--num-ctx 8192",
    ]);
    // A single set field → a single flag; max_tokens maps to --num-predict.
    expect(paramFlags({ max_tokens: 256 })).toEqual(["--num-predict 256"]);
  });
});

describe("shellQuote", () => {
  it("leaves shell-safe values (incl. model tags) bare", () => {
    expect(shellQuote("qwen2.5-coder:7b")).toBe("qwen2.5-coder:7b");
    expect(shellQuote("easy-coding")).toBe("easy-coding");
  });
  it("quotes values with spaces or parens", () => {
    expect(shellQuote("my model (q4)")).toBe("'my model (q4)'");
  });
});

describe("modeFrom", () => {
  it("maps the native/prompt boolean pair to the CLI --mode value", () => {
    expect(modeFrom(true, false)).toBe("native");
    expect(modeFrom(false, true)).toBe("prompt_based");
    expect(modeFrom(true, true)).toBe("both");
    expect(modeFrom(false, false)).toBe("native"); // default (native is the UI default)
  });
});

describe("buildRunCommand", () => {
  const base = { backend: "llama_cpp" as const, model: "qwen2.5:7b", collection: "medium-coding", isCustom: false, mode: "native" as const, thinking: "standard", k: 8 };

  it("builds a built-in `qm run` with exact flags", () => {
    expect(buildRunCommand({ ...base, tier: undefined }).command).toBe(
      "qm run --backend llama_cpp --model qwen2.5:7b --collection medium-coding --mode native --thinking standard --k 8",
    );
  });
  it("includes --tier only when set (auto → omitted, CLI uses the collection tier)", () => {
    expect(buildRunCommand({ ...base, tier: "hard" }).command).toContain("--tier hard");
    expect(buildRunCommand({ ...base, tier: undefined }).command).not.toContain("--tier");
  });
  it("switches to `qm test` + a .json collection for a custom collection, with a note", () => {
    const c = buildRunCommand({ ...base, collection: "my_suite", isCustom: true });
    expect(c.command).toContain("qm test");
    expect(c.command).toContain("--collection my_suite.json");
    expect(c.note).toMatch(/export/i);
  });
  it("marks incomplete + uses a shell-safe placeholder when no model is selected", () => {
    const c = buildRunCommand({ ...base, model: null });
    expect(c.incomplete).toBe(true);
    expect(c.command).toContain("--model YOUR_MODEL");
  });

  it("appends --max-steps, --decoy, and all set params", () => {
    const c = buildRunCommand({ ...base, maxSteps: 6, decoy: 2, params: { temperature: 0.4, seed: 7 } });
    expect(c.command).toContain("--max-steps 6");
    expect(c.command).toContain("--decoy 2");
    expect(c.command).toContain("--temperature 0.4");
    expect(c.command).toContain("--seed 7");
    // Unset params never appear.
    expect(c.command).not.toContain("--top-p");
  });
});

describe("buildPromptCommand", () => {
  it("builds `qm prompt` with params + a stdin note (Workspace's real equivalent)", () => {
    const c = buildPromptCommand({ backend: "llama_cpp", model: "qwen2.5:7b", params: { temperature: 0.8 } });
    expect(c.command).toBe("qm prompt --backend llama_cpp --model qwen2.5:7b --temperature 0.8");
    expect(c.note).toMatch(/stdin/);
  });
});

describe("buildRunHistory", () => {
  it("is a runnable save + re-assess chain", () => {
    const c = buildRunHistory({ backend: "llama_cpp", model: "qwen2.5:7b", collection: "easy-coding", isCustom: false, profile: "general-agent" });
    expect(c.command).toBe(
      "qm run --backend llama_cpp --model qwen2.5:7b --collection easy-coding --save-report run.json && qm report --report run.json --profile general-agent",
    );
  });
});

describe("buildCliffCommand", () => {
  it("builds `qm cliff` from probe settings", () => {
    expect(
      buildCliffCommand({ backend: "llama_cpp", model: "qwen2.5-coder:7b", collection: "easy-coding", maxTokens: 3000, steps: 3, source: "corporate_policy" }).command,
    ).toBe("qm cliff --backend llama_cpp --model qwen2.5-coder:7b --collection easy-coding --max-tokens 3000 --steps 3 --source corporate_policy");
  });
});

describe("buildReportCommand", () => {
  const profile = {
    id: "coding-agent",
    name: "Coding Agent",
    min_pass_k: 0.9,
    max_avg_steps: 8,
    max_ms_per_step: null,
    min_context_tokens: 16000,
    forbid_infinite_loop: true,
    forbid_hallucinated_completion: true,
    require_full_vram: false,
    require_native_fc: true,
    required_tier: "medium",
  };

  it("writes a profile.json carrying EVERY page threshold, then grades against it", () => {
    const c = buildReportCommand(profile);
    // The chain: write the exact thresholds → re-assess the saved run against them.
    const m = c.command.match(/^printf '%s' '(.*)' > profile\.json && qm report --report run\.json --profile profile\.json$/);
    expect(m).not.toBeNull();
    // Every threshold the page shows is in the emitted JSON, verbatim.
    expect(JSON.parse(m![1])).toEqual(profile);
    expect(c.note).toMatch(/--save-report/);
  });

  it("omits required_tier when the profile has none (old profiles stay valid)", () => {
    const { required_tier: _drop, ...noTier } = profile;
    const c = buildReportCommand(noTier);
    expect(c.command).not.toContain("required_tier");
  });
});


describe("buildCliffCommand — thinking preset", () => {
  it("emits --thinking only for a thinking probe, so the command reproduces the GUI run", () => {
    const base = { backend: "llama_cpp" as const, model: "qwen3.5-9b-q4", collection: "medium-coding-v2", maxTokens: 9216, steps: 4, source: "corporate_policy" };
    expect(buildCliffCommand(base).command).not.toContain("--thinking");
    expect(buildCliffCommand({ ...base, thinking: "deep" }).command).toContain("--thinking deep");
  });
});

import { describe, it, expect } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { VerdictTable } from "../components/VerdictTable";
import type { ModelVerdict } from "../../../shared/ipc/eval/readiness";

const GIB = 1024 ** 3;

const VERDICTS: ModelVerdict[] = [
  {
    model: "qwen2.5-coder",
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
      blocking: ["pass^k 0.40 < 0.80 required", "loops on 2 runs"],
      conditions: [],
      path: "prompt_based",
    },
  },
  {
    model: "mistral-nemo",
    backend: "vllm",
    verdict: {
      status: "conditional",
      blocking: [],
      conditions: ["slow: 8400ms/step > 5000ms target"],
      path: "prompt_based",
    },
  },
];

describe("VerdictTable", () => {
  it("renders a status badge per model with the measured path label", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    expect(screen.getByTestId("readiness-badge-ready")).toHaveTextContent("READY");
    expect(screen.getByTestId("readiness-badge-not_ready")).toHaveTextContent("NOT READY");
    expect(screen.getByTestId("readiness-badge-conditional")).toHaveTextContent("CONDITIONAL");
    // Path transparency: prompt-based, never silently presented as native.
    expect(screen.getByTestId("readiness-row-qwen2.5-coder")).toHaveTextContent("Prompt-Based");
  });

  it("shows the exact interpolated blocking math, not a bare 'failed'", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    const row = screen.getByTestId("readiness-row-phi3.5");
    expect(row).toHaveTextContent("✗ pass^k 0.40 < 0.80 required");
    expect(row).toHaveTextContent("✗ loops on 2 runs");
  });

  it("renders BLOCKING badges with a ✗ failure marker (never a literal 'X') and the real run counts in Details", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    const row = screen.getByTestId("readiness-row-phi3.5");
    // The visible badge is a category tag with a ✗ cross — not an ambiguous capital "X".
    expect(row).toHaveTextContent("BLOCKING: [✗ Reliability] [✗ Loops]");
    expect(row).not.toHaveTextContent("[X Reliability]");
    // The Details line surfaces the exact measured counts the backend reported, never "some runs".
    expect(row).toHaveTextContent("Pass^k (0.40) < 0.80");
    expect(row).toHaveTextContent("Loops on 2 runs");
  });

  it("omits the memory pill entirely when memory is unmeasured — no bare 'VRAM: N/A'", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    // phi3.5 is not_ready with no measured memory (a single-model / run-error-shaped row): the
    // row must read just its BLOCKING breakdown, never "VRAM: N/A |" (the reported display bug).
    const row = screen.getByTestId("readiness-row-phi3.5");
    expect(row).not.toHaveTextContent("VRAM: N/A");
    expect(row).toHaveTextContent("BLOCKING:");
  });

  it("VISIBLY renders the memory breakdown (weights + cache @ ctx vs cap), not just a bare total", () => {
    // The breakdown the hidden MemoryLine always computed must be user-visible —
    // "test pass ≠ data quality pass": a hidden node satisfies vitest, not a user.
    render(<VerdictTable verdicts={VERDICTS} />);
    const breakdown = within(screen.getByTestId("readiness-row-qwen2.5-coder")).getByTestId("memory-breakdown-visible");
    expect(breakdown).not.toHaveClass("hidden");
    expect(breakdown).toHaveTextContent("6.0 GB (5.0 model + 1.0 cache @ 8k ctx) < 24.0 GB cap · fits");
  });

  it("VISIBLY labels an estimated fit — the backend's 'UI labels the fit estimated' promise on screen", () => {
    const estimated: ModelVerdict[] = [
      {
        ...VERDICTS[0],
        memory: { ...VERDICTS[0].memory!, estimated: true },
      },
    ];
    render(<VerdictTable verdicts={estimated} />);
    const est = screen.getByTestId("memory-estimated-visible");
    expect(est).not.toHaveClass("hidden");
    expect(est).toHaveTextContent("est.");
    // ...and the exact fit line carries the est. marker too.
    expect(screen.getByTestId("memory-breakdown-visible")).toHaveTextContent("· est.");
  });

  it("VISIBLY renders the verdict's exact backend reasons, not only the lossy category pills", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    // The not_ready row: the verbatim blocking strings the verdict gated on.
    const blocked = within(screen.getByTestId("readiness-row-phi3.5")).getByTestId("verdict-reasons-visible");
    expect(blocked).not.toHaveClass("hidden");
    expect(blocked).toHaveTextContent("✗ pass^k 0.40 < 0.80 required");
    expect(blocked).toHaveTextContent("✗ loops on 2 runs");
    // The conditional row: the verbatim soft condition.
    const cond = within(screen.getByTestId("readiness-row-mistral-nemo")).getByTestId("verdict-reasons-visible");
    expect(cond).toHaveTextContent("! slow: 8400ms/step > 5000ms target");
  });

  it("shows a thinking segment for a reasoning verdict and nothing for a terse one", () => {
    const thinking: ModelVerdict = {
      model: "qwen3-thinking",
      backend: "llama_cpp",
      verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
      pass_k: 0.9,
      is_thinking: true,
      think_preset: "deep",
      ctx_ceiling: 32768,
      cpu_offloaded: true,
    };
    render(<VerdictTable verdicts={[thinking, ...VERDICTS]} />);
    const row = screen.getByTestId("readiness-row-qwen3-thinking");
    // The reasoning-budget items live inside one grouped pill (thinking + ctx + offload
    // together), so the segment reads as a unit instead of blending into the metric row.
    const group = within(row).getByTestId("metric-thinking-group");
    expect(within(group).getByTestId("metric-thinking")).toHaveTextContent("thinking: Deep");
    expect(within(group).getByTestId("metric-ctx")).toHaveTextContent("ctx 32k");
    expect(within(group).getByTestId("metric-offload")).toBeInTheDocument();
    // A terse model shows no thinking metric at all (no N/A).
    const terse = screen.getByTestId("readiness-row-qwen2.5-coder");
    expect(within(terse).queryByTestId("metric-thinking")).toBeNull();
  });

  it("categorizes every backend blocking reason — never the meaningless 'System' fallback", () => {
    const verdicts: ModelVerdict[] = [
      {
        model: "phi3.5",
        backend: "llama_cpp",
        verdict: {
          status: "not_ready",
          blocking: [
            "false 'done' on 2 runs",
            "native tool-calling required but not supported/measured on this backend",
            "partial offload → severe slowdown",
            "run error: the server timed out",
          ],
          conditions: [],
          path: "prompt_based",
        },
      },
    ];
    render(<VerdictTable verdicts={verdicts} />);
    const row = screen.getByTestId("readiness-row-phi3.5");
    // Each reason maps to a real category, not the catch-all "System".
    expect(row).toHaveTextContent("BLOCKING: [✗ Reliability] [✗ Native FC] [✗ Hardware] [✗ Run Error]");
    expect(row).not.toHaveTextContent("System");
    // The Details line still surfaces the exact backend reasons (with their counts/messages).
    expect(row).toHaveTextContent("False 'done' on 2 runs");
    expect(row).toHaveTextContent("Run error: the server timed out");
  });

  it("renders conditions as amber notes and a clean Ready row as 'meets all criteria'", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    expect(screen.getByTestId("readiness-row-mistral-nemo")).toHaveTextContent("! slow: 8400ms/step > 5000ms target");
    expect(screen.getByTestId("readiness-row-qwen2.5-coder")).toHaveTextContent("Meets all criteria");
  });

  it("renders the memory footprint for a measured model and N/A for a remote backend", () => {
    render(<VerdictTable verdicts={VERDICTS} />);
    // llama.cpp with a measured profile → weights + cache vs cap, "fits".
    expect(screen.getByTestId("readiness-row-qwen2.5-coder")).toHaveTextContent(
      "VRAM: 6.0 GB (5.0 model + 1.0 cache @ 8k ctx) < 24.0 GB cap · fits",
    );
    // A remote backend runs on another machine → honest N/A, never a guessed fit.
    expect(screen.getByTestId("readiness-row-mistral-nemo")).toHaveTextContent("VRAM fit: N/A (remote backend)");
    // A measured-but-exact profile shows no estimate caveat.
    expect(screen.queryByTestId("vram-estimated")).not.toBeInTheDocument();
  });

  it("labels the VRAM line as a conservative estimate when KV head count was defaulted", () => {
    const est: ModelVerdict[] = [
      {
        model: "qwen3.5",
        backend: "llama_cpp",
        verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
        memory: { weights_bytes: 5 * GIB, kv_cache_bytes: 2 * GIB, total_bytes: 7 * GIB, cap_bytes: 24 * GIB, context_length: 8192, fits: true, pressure: false, estimated: true },
      },
    ];
    render(<VerdictTable verdicts={est} />);
    const row = screen.getByTestId("readiness-row-qwen3.5");
    expect(row).toHaveTextContent("· est."); // hidden machine-readable string
    expect(within(row).getByTestId("vram-estimated")).toBeInTheDocument();
  });

  it("shows REAL measured metrics (Pass^k / steps / effort) — values or N/A, never fabricated", () => {
    const real: ModelVerdict[] = [
      {
        model: "llama3.2:3b",
        backend: "llama_cpp",
        verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
        pass_k: 1.0,
        avg_steps: 1.0,
        effort: 29,
        quantization: "Q4_K_M", // real backend value
        cliff: { status: "Collapsed", depth: 12000 }, // measured by the Context-Cliff probe
        memory: null, // VRAM not measured (no cap)
      },
      {
        model: "phi3.5",
        backend: "llama_cpp",
        verdict: { status: "not_ready", blocking: ["pass^k 0.40 < 0.80 required"], conditions: [], path: "prompt_based" },
        pass_k: 0.4, // steps/effort undefined → N/A
      },
    ];
    render(<VerdictTable verdicts={real} />);

    const ready = screen.getByTestId("readiness-row-llama3.2:3b");
    expect(within(ready).getByTestId("metric-passk")).toHaveTextContent("100%");
    expect(within(ready).getByTestId("metric-steps")).toHaveTextContent("1.0");
    expect(within(ready).getByTestId("metric-effort")).toHaveTextContent("29 tok");
    expect(within(ready).getByTestId("metric-cliff")).toHaveTextContent("Collapsed at 12,000 tok"); // measured collapse
    expect(ready).toHaveTextContent("Q4_K_M"); // real quant, not a per-family guess
    // VRAM was NOT measured → must NOT claim it fits.
    expect(ready).not.toHaveTextContent("Fits completely in VRAM");

    const weak = screen.getByTestId("readiness-row-phi3.5");
    expect(within(weak).getByTestId("metric-passk")).toHaveTextContent("40%");
    expect(within(weak).getByTestId("metric-steps")).toHaveTextContent("N/A");
    expect(within(weak).getByTestId("metric-effort")).toHaveTextContent("N/A");
    expect(within(weak).getByTestId("metric-cliff")).toHaveTextContent("N/A"); // no probe → N/A, not fabricated
    // The metrics must be VISIBLE even on a not_ready row (the bug: they were in a
    // `hidden` wrapper, so the row showed only a red ✗ with no numbers).
    expect(within(weak).getByTestId("readiness-metrics")).not.toHaveClass("hidden");
  });

  it("labels memory 'Unified memory' on a unified-memory machine, 'VRAM' otherwise", () => {
    // Default (discrete GPU): says VRAM.
    const { rerender } = render(<VerdictTable verdicts={VERDICTS} />);
    const readyDiscrete = screen.getByTestId("readiness-row-qwen2.5-coder");
    expect(readyDiscrete).toHaveTextContent("VRAM:");
    expect(readyDiscrete).toHaveTextContent("✓ Fits in VRAM");
    // Apple Silicon / unified: no discrete VRAM — say "Unified memory".
    rerender(<VerdictTable verdicts={VERDICTS} unified />);
    const readyUnified = screen.getByTestId("readiness-row-qwen2.5-coder");
    expect(readyUnified).toHaveTextContent("Unified memory:");
    expect(readyUnified).toHaveTextContent("✓ Fits in Unified memory");
    // (The visible labels switch to "Unified memory"; a hidden MemoryLine — kept only for
    // Vitest — may still contain "VRAM", so we don't assert its absence row-wide.)
  });

  it("renders a broken baseline as 'fails from start', never a depth", () => {
    const broke: ModelVerdict[] = [
      {
        model: "broke:9b",
        backend: "llama_cpp",
        verdict: { status: "not_ready", blocking: [], conditions: [], path: "native_fc" },
        pass_k: 1.0,
        cliff: { status: "Broken", tested: 388 },
      },
    ];
    render(<VerdictTable verdicts={broke} />);
    const cell = within(screen.getByTestId("readiness-row-broke:9b")).getByTestId("metric-cliff");
    expect(cell).toHaveTextContent("fails from start");
    expect(cell).not.toHaveTextContent("388");
  });

  it("renders a probed-but-no-cliff result as '✓ No cliff (≥tested)', not N/A", () => {
    const held: ModelVerdict[] = [
      {
        model: "qwen3.5:9b",
        backend: "llama_cpp",
        verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
        pass_k: 1.0,
        cliff: { status: "NoCliff", tested: 4000 },
      },
    ];
    render(<VerdictTable verdicts={held} />);
    expect(within(screen.getByTestId("readiness-row-qwen3.5:9b")).getByTestId("metric-cliff")).toHaveTextContent(
      "✓ No cliff (≥4,000 tok)",
    );
  });

  it("never guesses a quant — an unknown name renders a dash, not a family default", () => {
    render(
      <VerdictTable
        verdicts={[{ model: "qwen2.5-coder", backend: "llama_cpp", verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" } }]}
      />,
    );
    // The old code fabricated "q5_k_m" for any qwen; an unknown quant is now an honest "—".
    expect(screen.getByTestId("readiness-row-qwen2.5-coder")).not.toHaveTextContent(/q5_k_m/i);
  });

  it("shows graduated readiness (cleared / requires tier) when the profile is tiered", () => {
    render(
      <VerdictTable
        verdicts={[
          {
            model: "big-model",
            backend: "llama_cpp",
            verdict: {
              status: "not_ready",
              blocking: ["cleared Medium; this profile requires Extreme"],
              conditions: [],
              path: "native_fc",
              required_tier: "extreme",
              cleared_tier: "medium",
            },
          },
        ]}
      />,
    );
    expect(screen.getByTestId("tier-line")).toHaveTextContent("cleared Medium / requires Extreme");
  });

  it("hides the tier line for an untiered (Easy / absent) profile", () => {
    render(<VerdictTable verdicts={VERDICTS} />); // these verdicts carry no required_tier
    expect(screen.queryByTestId("tier-line")).toBeNull();
  });

  it("renders BOTH paths of a dual-measured model as two rows, and the toggle hides the native one", () => {
    // One model measured on both paths → two ModelVerdicts sharing model+backend, distinct path.
    const dual: ModelVerdict[] = [
      {
        model: "qwen2.5-coder",
        backend: "llama_cpp",
        verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" },
        pass_k: 0.82,
      },
      {
        model: "qwen2.5-coder",
        backend: "llama_cpp",
        verdict: { status: "ready", blocking: [], conditions: [], path: "prompt_based" },
        pass_k: 0.74,
      },
    ];
    const { rerender } = render(<VerdictTable verdicts={dual} showNativeFc={true} />);
    // Both path labels appear (two rows for the same model, keyed by path so they don't collide).
    expect(screen.getByText(/Native FC/)).toBeInTheDocument();
    expect(screen.getByText(/Prompt-Based/)).toBeInTheDocument();

    // Toggle OFF → the native row is filtered out, the prompt-based row remains.
    rerender(<VerdictTable verdicts={dual} showNativeFc={false} />);
    expect(screen.queryByText(/Native FC/)).toBeNull();
    expect(screen.getByText(/Prompt-Based/)).toBeInTheDocument();
  });
});

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TtftBreakdown } from "../TtftBreakdown";

describe("TtftBreakdown", () => {
  it("renders load + prefill + remainder segments with a prompt-token caption", () => {
    render(<TtftBreakdown ttftMs={820} stats={{ load_ms: 540, prompt_eval_ms: 210, prompt_eval_count: 128 }} />);
    expect(screen.getByTestId("ttft-breakdown")).toBeInTheDocument();
    expect(screen.getByTestId("ttft-seg-load")).toBeInTheDocument();
    expect(screen.getByTestId("ttft-seg-prefill")).toBeInTheDocument();
    expect(screen.getByTestId("ttft-seg-remainder")).toBeInTheDocument();
    expect(screen.getByText(/128 prompt tokens/)).toBeInTheDocument();
  });

  it("shows 'not available' when the backend reports nothing", () => {
    render(<TtftBreakdown ttftMs={500} stats={{}} />);
    expect(screen.getByTestId("ttft-na")).toBeInTheDocument();
    expect(screen.queryByTestId("ttft-breakdown")).toBeNull();
  });

  it("omits the load segment for a backend that reports only prefill", () => {
    render(<TtftBreakdown ttftMs={300} stats={{ prompt_eval_ms: 210 }} />);
    expect(screen.queryByTestId("ttft-seg-load")).toBeNull();
    expect(screen.getByTestId("ttft-seg-prefill")).toBeInTheDocument();
  });

  it("derives prefill tok/s, and renders the 0/0 full-cache-hit case as 'cache hit'", () => {
    // Normal prefill → a throughput number.
    const { rerender } = render(
      <TtftBreakdown ttftMs={820} stats={{ prompt_eval_ms: 200, prompt_eval_count: 100 }} />,
    );
    expect(screen.getByText(/tok\/s prefill/)).toBeInTheDocument();
    // Full prefix-cache hit: 0 tokens in ~0 ms → no NaN/∞, render "cache hit — no prefill".
    rerender(<TtftBreakdown ttftMs={50} stats={{ prompt_eval_ms: 0, prompt_eval_count: 0 }} />);
    expect(screen.getByText(/cache hit — no prefill/)).toBeInTheDocument();
    expect(screen.queryByText(/NaN|Infinity/)).toBeNull();
  });

  it("shows the prefix-cache reuse line for llama.cpp — and distinguishes a measured 0 from absent", () => {
    // Warm llama run: cache_n=240 reused, prompt_eval_count=10 recomputed (prompt_n) → total 250.
    const { rerender } = render(
      <TtftBreakdown ttftMs={820} stats={{ prompt_eval_ms: 30, prompt_eval_count: 10, cache_n: 240 }} />,
    );
    expect(screen.getByTestId("ttft-prefix-cache")).toHaveTextContent("240 reused / 10 recomputed");
    // COLD llama run: cache_n is a measured 0, the whole 250 recomputed → present, "0 reused".
    rerender(<TtftBreakdown ttftMs={820} stats={{ prompt_eval_ms: 300, prompt_eval_count: 250, cache_n: 0 }} />);
    expect(screen.getByTestId("ttft-prefix-cache")).toHaveTextContent("0 reused / 250 recomputed");
    // A backend without the feature: cache_n absent → the line is ABSENT (not a false "0 reused").
    rerender(<TtftBreakdown ttftMs={820} stats={{ prompt_eval_ms: 300, prompt_eval_count: 250 }} />);
    expect(screen.queryByTestId("ttft-prefix-cache")).toBeNull();
  });
});

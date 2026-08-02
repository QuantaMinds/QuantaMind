import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { CompareColumn } from "../components/CompareColumn";
import type { CompareRow } from "../state/compareStore";

const ROW = (over: Partial<CompareRow> = {}): CompareRow => ({
  model: "llama3.2:1b", modelId: null, status: "pending", output: "",
  metrics: null, error: null, startedAt: null, endedAt: null,
  ...over,
});

describe("CompareColumn", () => {
  it("renders the model name and pending status", () => {
    render(<CompareColumn row={ROW()} />);
    expect(screen.getByText("llama3.2:1b")).toBeInTheDocument();
    expect(screen.getByTestId("compare-status-llama3.2:1b")).toHaveTextContent("Waiting");
  });

  it("renders streaming output and Running status", () => {
    render(<CompareColumn row={ROW({ status: "running", output: "Hello world" })} />);
    expect(screen.getByTestId("compare-status-llama3.2:1b")).toHaveTextContent("Running");
    expect(screen.getByTestId("compare-output-llama3.2:1b")).toHaveTextContent("Hello world");
  });

  it("renders metrics line only when status is done", () => {
    const row = ROW({ status: "done", output: "ok",
      metrics: { ttft_ms: 142, tokens_per_sec: 38.2, token_count: 218 } });
    render(<CompareColumn row={row} />);
    expect(screen.getByTestId("compare-metrics-llama3.2:1b")).toHaveTextContent(
      "TTFT 142ms · 38.2 tok/s · 218 tokens",
    );
  });

  it("appends a llama.cpp prefix-cache segment, but stays byte-identical for a no-cache run", () => {
    // llama.cpp: cache_n=240 reused, prompt_eval_count=10 recomputed → total 250.
    const llama = ROW({ status: "done", output: "ok",
      metrics: { ttft_ms: 142, tokens_per_sec: 38.2, token_count: 218,
        stats: { cache_n: 240, prompt_eval_count: 10 } } });
    render(<CompareColumn row={llama} />);
    expect(screen.getByTestId("compare-metrics-llama3.2:1b")).toHaveTextContent(
      "TTFT 142ms · 38.2 tok/s · 218 tokens · cache 240/250 reused",
    );
  });

  it("omits the cache segment for a backend without it (stats present, no cache_n)", () => {
    const noCache = ROW({ status: "done", output: "ok",
      metrics: { ttft_ms: 142, tokens_per_sec: 38.2, token_count: 218,
        stats: { prompt_eval_count: 250 } } });
    render(<CompareColumn row={noCache} />);
    // Byte-identical to the no-stats case — never a false "cache 0/250".
    expect(screen.getByTestId("compare-metrics-llama3.2:1b")).toHaveTextContent(
      "TTFT 142ms · 38.2 tok/s · 218 tokens",
    );
    expect(screen.queryByText(/cache/)).toBeNull();
  });

  it("renders the error block when status is error", () => {
    render(<CompareColumn row={ROW({ status: "error", error: { kind: "inference", message: "HTTP 500" } })} />);
    expect(screen.getByTestId("compare-error-llama3.2:1b")).toHaveTextContent("inference: HTTP 500");
  });

  it("renders Cancelled status without metrics", () => {
    render(<CompareColumn row={ROW({ status: "cancelled" })} />);
    expect(screen.getByTestId("compare-status-llama3.2:1b")).toHaveTextContent("Cancelled");
    expect(screen.queryByTestId("compare-metrics-llama3.2:1b")).toBeNull();
  });
});

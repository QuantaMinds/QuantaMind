import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { KvCeilingBars } from "../kv/KvCeilingBars";
import { useCliffStore } from "../../../eval/state/cliffStore";
import { useKvCeilings } from "../../hooks/useKvCeilings";
import type { ModelDims, CtxCeilings } from "../../../../shared/ipc/system/inspect";

vi.mock("../../hooks/useKvCeilings", () => ({ useKvCeilings: vi.fn() }));

const dims = (ctxMax: number, kvEstimated = false): ModelDims => ({
  layers: 32,
  head_count: 40,
  head_count_kv: 8,
  embedding_length: 5120,
  context_length: ctxMax,
  kv_estimated: kvEstimated,
});

function mockCeilings(ceilings: CtxCeilings | null, ctxMax = 262_144, kvEstimated = false) {
  vi.mocked(useKvCeilings).mockReturnValue({ dims: ceilings ? dims(ctxMax, kvEstimated) : null, ceilings });
}

beforeEach(() => {
  useCliffStore.setState({ results: {} });
  vi.clearAllMocks();
});

describe("KvCeilingBars", () => {
  it("renders three precision rows with monotonic widths and captions", () => {
    mockCeilings({ f16: 14_848, q8: 29_696, q4: 59_648 });
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    // Each precision labelled and captioned with its ceiling.
    expect(screen.getByTestId("kv-ceiling-f16")).toHaveTextContent("14,848 ctx");
    expect(screen.getByTestId("kv-ceiling-q8")).toHaveTextContent("29,696 ctx");
    expect(screen.getByTestId("kv-ceiling-q4")).toHaveTextContent("59,648 ctx");
    // q8 fills more cells than f16, q4 more than q8 (shared scale, more context as cache shrinks).
    const filled = (id: string) => (screen.getByTestId(id).textContent!.match(/█/g) ?? []).length;
    expect(filled("kv-ceiling-f16")).toBeLessThan(filled("kv-ceiling-q8"));
    expect(filled("kv-ceiling-q8")).toBeLessThan(filled("kv-ceiling-q4"));
  });

  it("carries the Q4 dual caveat (quality AND speed) and the never-auto-launch note", () => {
    mockCeilings({ f16: 10_000, q8: 20_000, q4: 40_000 });
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    const el = screen.getByTestId("kv-ceilings");
    expect(el).toHaveTextContent("slower at long context");
    expect(el).toHaveTextContent("never auto-launches a q4_0 cache");
  });

  it("clamps a ceiling above the model's declared max and labels it '(model max)'", () => {
    // q4 = 300k but the model only supports 262,144 → capped, tagged.
    mockCeilings({ f16: 100_000, q8: 200_000, q4: 300_000 }, 262_144);
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    expect(screen.getByTestId("kv-ceiling-q4")).toHaveTextContent("262,144 ctx (model max)");
  });

  it("shows a per-precision 'Not available' when one ceiling is unmeasurable", () => {
    mockCeilings({ f16: 14_848, q8: 29_696, q4: null });
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    expect(screen.getByTestId("kv-ceiling-q4")).toHaveTextContent("Not available");
  });

  it("shows a whole-panel 'Not available' when dims/ceilings can't be measured (e.g. backend unreachable)", () => {
    mockCeilings(null);
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={null} totalBytes={16e9} />);
    expect(screen.getByTestId("kv-ceilings")).toHaveTextContent("Not available");
  });

  it("marks the meters '~ estimated' when the model didn't report its KV head count (conservative, not silently wrong)", () => {
    mockCeilings({ f16: 14_848, q8: 29_696, q4: 59_648 }, 262_144, true);
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    expect(screen.getByTestId("kv-ceilings")).toHaveTextContent("~ estimated");
  });

  it("places the cliff marker from the cliff store", () => {
    useCliffStore.setState({ results: { finance: { m: 20_000 } } });
    mockCeilings({ f16: 14_848, q8: 29_696, q4: 59_648 });
    render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} />);
    // The cliff title appears somewhere in the bars (marker rendered inline).
    expect(document.querySelector('[title*="cliff edge ≈20000"]')).not.toBeNull();
  });

  it("labels unified vs discrete memory in the caption", () => {
    mockCeilings({ f16: 10_000, q8: 20_000, q4: 40_000 });
    const { rerender } = render(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} unified />);
    expect(screen.getByTestId("kv-ceilings")).toHaveTextContent("unified memory");
    rerender(<KvCeilingBars modelName="m" backend="ollama" modelBytes={9e9} totalBytes={16e9} unified={false} />);
    expect(screen.getByTestId("kv-ceilings")).toHaveTextContent("VRAM");
  });
});

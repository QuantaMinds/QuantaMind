import { describe, it, expect } from "vitest";
import { backendRunHint } from "../state/runHint";

const h = (
  ollama: boolean | null,
  llama: boolean | null,
  mlx: boolean | null,
  vllm: boolean | null = null,
  sglang: boolean | null = null,
) => ({ ollama, llama, mlx, vllm, sglang });

describe("backendRunHint (no-fallback block)", () => {
  it("blocks MLX with a start hint until its server is healthy", () => {
    expect(backendRunHint("mlx", h(true, null, false))).toBe("Start the MLX backend to run this model");
    expect(backendRunHint("mlx", h(true, null, null))).toBe("Start the MLX backend to run this model");
    expect(backendRunHint("mlx", h(false, false, true))).toBeNull();
  });

  it("blocks llama.cpp until healthy", () => {
    expect(backendRunHint("llama_cpp", h(true, false, null))).toBe("Start llama.cpp to run this model");
    expect(backendRunHint("llama_cpp", h(false, true, false))).toBeNull();
  });

  it("blocks Ollama only when explicitly unhealthy (null = still checking)", () => {
    expect(backendRunHint("ollama", h(false, null, null))).toBe("Start Ollama first");
    expect(backendRunHint("ollama", h(null, null, null))).toBeNull();
    expect(backendRunHint("ollama", h(true, null, null))).toBeNull();
  });

  it("blocks the remote backends until their endpoint is reachable", () => {
    expect(backendRunHint("vllm", h(null, null, null, false))).toBe(
      "Set the vLLM server URL in Settings and start it",
    );
    expect(backendRunHint("vllm", h(null, null, null, null))).toBe(
      "Set the vLLM server URL in Settings and start it",
    );
    expect(backendRunHint("vllm", h(null, null, null, true))).toBeNull();
    expect(backendRunHint("sglang", h(null, null, null, null, false))).toBe(
      "Set the SGLang server URL in Settings and start it",
    );
    expect(backendRunHint("sglang", h(null, null, null, null, true))).toBeNull();
  });
});

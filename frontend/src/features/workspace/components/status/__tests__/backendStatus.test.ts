import { describe, it, expect } from "vitest";
import { backendStatus } from "../backendStatus";

describe("backendStatus", () => {
  it("names MLX (not Ollama) when MLX is the active backend", () => {
    const s = backendStatus("mlx", null, null, true, null, null, "stub-mlx");
    expect(s.aria).toBe("MLX health");
    expect(s.running).toBe(true);
    expect(s.label).toBe("MLX · running (stub-mlx)");
  });

  it("shows MLX not running when its server is down — never 'Ollama not running'", () => {
    const s = backendStatus("mlx", null, null, false, null, null, null);
    expect(s.aria).toBe("MLX health");
    expect(s.running).toBe(false);
    expect(s.label).toBe("MLX · not running");
  });

  it("tracks llama.cpp run state and names the model", () => {
    expect(backendStatus("llama_cpp", null, true, null, null, null, "phi3").label).toBe(
      "llama.cpp · running (phi3)",
    );
    expect(backendStatus("llama_cpp", null, false, null, null, null, null).label).toBe(
      "llama.cpp · not started",
    );
  });

  it("shows the remote backends as connected / not reachable", () => {
    const vllmUp = backendStatus("vllm", null, null, null, true, null, "qwen");
    expect(vllmUp.aria).toBe("vLLM health");
    expect(vllmUp.running).toBe(true);
    expect(vllmUp.label).toBe("vLLM · connected (qwen)");
    expect(backendStatus("vllm", null, null, null, false, null, null).label).toBe("vLLM · not reachable");
    expect(backendStatus("sglang", null, null, null, null, true, null).label).toBe("SGLang · connected");
    expect(backendStatus("sglang", null, null, null, null, null, null).label).toBe("SGLang · not reachable");
  });

  it("uses polled Ollama health with the version when connected", () => {
    const up = backendStatus("ollama", { available: true, version: "0.1.32" }, null, null, null, null, null);
    expect(up.aria).toBe("Ollama health");
    expect(up.running).toBe(true);
    expect(up.label).toMatch(/connected.*0\.1\.32/);
    expect(backendStatus("ollama", null, null, null, null, null, null).label).toBe("checking…");
    expect(
      backendStatus("ollama", { available: false, version: null }, null, null, null, null, null).label,
    ).toBe("Ollama not running");
  });
});

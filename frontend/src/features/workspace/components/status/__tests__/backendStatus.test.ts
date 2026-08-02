import { describe, it, expect } from "vitest";
import { backendStatus } from "../backendStatus";

describe("backendStatus", () => {
  it("tracks llama.cpp run state and names the model", () => {
    expect(backendStatus("llama_cpp", true, null, null, "phi3").label).toBe("llama.cpp · running (phi3)");
    expect(backendStatus("llama_cpp", false, null, null, null).label).toBe("llama.cpp · not started");
    expect(backendStatus("llama_cpp", true, null, null, null).aria).toBe("llama.cpp health");
  });

  it("shows the remote backends as connected / not reachable", () => {
    const vllmUp = backendStatus("vllm", null, true, null, "qwen");
    expect(vllmUp.aria).toBe("vLLM health");
    expect(vllmUp.running).toBe(true);
    expect(vllmUp.label).toBe("vLLM · connected (qwen)");
    expect(backendStatus("vllm", null, false, null, null).label).toBe("vLLM · not reachable");
    expect(backendStatus("sglang", null, null, true, null).label).toBe("SGLang · connected");
    expect(backendStatus("sglang", null, null, null, null).label).toBe("SGLang · not reachable");
  });

  it("never reports a backend as running on another backend's health", () => {
    // A healthy remote must not make the local llama.cpp dot go green, or the
    // status bar would claim a server is up that isn't.
    expect(backendStatus("llama_cpp", null, true, true, null).running).toBe(false);
    expect(backendStatus("vllm", true, null, null, null).running).toBe(false);
  });
});

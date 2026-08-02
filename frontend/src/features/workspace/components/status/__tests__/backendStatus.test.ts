import { describe, it, expect } from "vitest";
import { backendStatus } from "../backendStatus";

describe("backendStatus", () => {
  it("tracks llama.cpp run state and names the model", () => {
    expect(backendStatus("llama_cpp", true, null, "phi3").label).toBe("llama.cpp · running (phi3)");
    expect(backendStatus("llama_cpp", false, null, null).label).toBe("llama.cpp · not started");
    expect(backendStatus("llama_cpp", true, null, null).aria).toBe("llama.cpp health");
  });

  it("shows the remote backend as connected / not reachable", () => {
    const up = backendStatus("vllm", null, true, "qwen");
    expect(up.aria).toBe("vLLM health");
    expect(up.running).toBe(true);
    expect(up.label).toBe("vLLM · connected (qwen)");
    expect(backendStatus("vllm", null, false, null).label).toBe("vLLM · not reachable");
  });

  it("never reports a backend as running on another backend's health", () => {
    // A healthy remote must not make the local llama.cpp dot go green, or the
    // status bar would claim a server is up that isn't.
    expect(backendStatus("llama_cpp", null, true, null).running).toBe(false);
    expect(backendStatus("vllm", true, null, null).running).toBe(false);
  });
});

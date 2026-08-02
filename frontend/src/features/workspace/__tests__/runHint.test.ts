import { describe, it, expect } from "vitest";
import { backendRunHint } from "../state/runHint";

const h = (
  llama: boolean | null,
  vllm: boolean | null = null,
  sglang: boolean | null = null,
) => ({ llama, vllm, sglang });

describe("backendRunHint (no-fallback block)", () => {
  it("blocks llama.cpp until its server is healthy", () => {
    expect(backendRunHint("llama_cpp", h(false))).toBe("Start llama.cpp to run this model");
    // null = still checking → still blocked, never an optimistic run.
    expect(backendRunHint("llama_cpp", h(null))).toBe("Start llama.cpp to run this model");
    expect(backendRunHint("llama_cpp", h(true))).toBeNull();
  });

  it("blocks the remote backends until their endpoint is reachable", () => {
    expect(backendRunHint("vllm", h(null, false))).toBe("Set the vLLM server URL in Settings and start it");
    expect(backendRunHint("vllm", h(null, null))).toBe("Set the vLLM server URL in Settings and start it");
    expect(backendRunHint("vllm", h(null, true))).toBeNull();
    expect(backendRunHint("sglang", h(null, null, null))).toBe("Set the SGLang server URL in Settings and start it");
    expect(backendRunHint("sglang", h(null, null, true))).toBeNull();
  });

  it("never lets one backend's health unblock another", () => {
    // A healthy remote must not unblock a llama.cpp run — the model can't run there.
    expect(backendRunHint("llama_cpp", h(false, true, true))).toBe("Start llama.cpp to run this model");
    expect(backendRunHint("vllm", h(true, false))).toBe("Set the vLLM server URL in Settings and start it");
  });
});

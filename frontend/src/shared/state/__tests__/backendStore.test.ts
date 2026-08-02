import { describe, it, expect, beforeEach } from "vitest";
import { useBackendStore } from "../backendStore";

beforeEach(() => {
  useBackendStore.setState({
    selectedBackend: "llama_cpp",
    llamaHealthy: null,
    vllmHealthy: null,
  });
});

describe("backendStore (global backend selection + health)", () => {
  it("defaults to llama.cpp with unknown (null) health", () => {
    const s = useBackendStore.getState();
    expect(s.selectedBackend).toBe("llama_cpp");
    expect(s.llamaHealthy).toBeNull();
    expect(s.llamaHealthy).toBeNull();
    expect(s.llamaHealthy).toBeNull();
  });

  it("setSelectedBackend switches the active backend", () => {
    useBackendStore.getState().setSelectedBackend("llama_cpp");
    expect(useBackendStore.getState().selectedBackend).toBe("llama_cpp");
    useBackendStore.getState().setSelectedBackend("vllm");
    expect(useBackendStore.getState().selectedBackend).toBe("vllm");
  });

  it("isHealthy reads the flag for the requested backend", () => {
    const { setLlamaHealthy, setVllmHealthy } = useBackendStore.getState();
    setLlamaHealthy(true);
    setVllmHealthy(false);
    const { isHealthy } = useBackendStore.getState();
    expect(isHealthy("llama_cpp")).toBe(true);
    expect(isHealthy("vllm")).toBe(false);
  });

  it("isHealthy returns null before the first probe", () => {
    expect(useBackendStore.getState().isHealthy("llama_cpp")).toBeNull();
  });
});

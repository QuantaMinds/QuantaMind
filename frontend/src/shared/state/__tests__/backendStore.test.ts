import { describe, it, expect, beforeEach } from "vitest";
import { useBackendStore } from "../backendStore";

beforeEach(() => {
  useBackendStore.setState({
    selectedBackend: "llama_cpp",
    ollamaHealthy: null,
    llamaHealthy: null,
    mlxHealthy: null,
    vllmHealthy: null,
    sglangHealthy: null,
  });
});

describe("backendStore (global backend selection + health)", () => {
  it("defaults to llama.cpp with unknown (null) health", () => {
    const s = useBackendStore.getState();
    expect(s.selectedBackend).toBe("llama_cpp");
    expect(s.ollamaHealthy).toBeNull();
    expect(s.llamaHealthy).toBeNull();
    expect(s.mlxHealthy).toBeNull();
  });

  it("setSelectedBackend switches the active backend", () => {
    useBackendStore.getState().setSelectedBackend("llama_cpp");
    expect(useBackendStore.getState().selectedBackend).toBe("llama_cpp");
    useBackendStore.getState().setSelectedBackend("mlx");
    expect(useBackendStore.getState().selectedBackend).toBe("mlx");
  });

  it("isHealthy reads the flag for the requested backend", () => {
    const { setOllamaHealthy, setLlamaHealthy, setMlxHealthy, setVllmHealthy, setSglangHealthy } =
      useBackendStore.getState();
    setOllamaHealthy(true);
    setLlamaHealthy(false);
    setMlxHealthy(true);
    setVllmHealthy(true);
    setSglangHealthy(false);
    const { isHealthy } = useBackendStore.getState();
    expect(isHealthy("ollama")).toBe(true);
    expect(isHealthy("llama_cpp")).toBe(false);
    expect(isHealthy("mlx")).toBe(true);
    expect(isHealthy("vllm")).toBe(true);
    expect(isHealthy("sglang")).toBe(false);
  });

  it("isHealthy returns null before the first probe", () => {
    expect(useBackendStore.getState().isHealthy("ollama")).toBeNull();
  });
});

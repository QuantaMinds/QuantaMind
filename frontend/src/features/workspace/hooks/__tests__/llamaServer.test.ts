import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

vi.mock("../../../../shared/ipc/models/llama_start", () => ({
  startLlamaServer: vi.fn(),
  stopLlamaServer: vi.fn(),
}));

import { startLlamaServer, stopLlamaServer } from "../../../../shared/ipc/models/llama_start";
import { useStartLlamaServer } from "../useStartLlamaServer";
import { useStopLlamaServer } from "../useStopLlamaServer";
import { useBackendStore } from "../../../../shared/state/backendStore";

beforeEach(() => {
  vi.mocked(startLlamaServer).mockReset();
  vi.mocked(stopLlamaServer).mockReset();
  useBackendStore.setState({ llamaHealthy: null });
});

describe("useStartLlamaServer", () => {
  it("marks llama healthy after a successful start with the model path", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "started", pid: 1, port: 8080 });
    const { result } = renderHook(() => useStartLlamaServer());
    await act(async () => { await result.current.start("/g/phi3.gguf", 16384); });
    expect(startLlamaServer).toHaveBeenCalledWith("/g/phi3.gguf", 16384);
    expect(useBackendStore.getState().llamaHealthy).toBe(true);
    expect(result.current.status).toBe("idle");
  });

  it("surfaces not_bundled without marking healthy", async () => {
    vi.mocked(startLlamaServer).mockResolvedValue({ status: "not_bundled", note: "no binary" });
    const { result } = renderHook(() => useStartLlamaServer());
    await act(async () => { await result.current.start("/g/x.gguf"); });
    expect(result.current.status).toBe("not_bundled");
    expect(result.current.error).toBe("no binary");
    expect(useBackendStore.getState().llamaHealthy).toBeNull();
  });
});

describe("useStopLlamaServer", () => {
  it("marks llama unhealthy after stop", async () => {
    useBackendStore.setState({ llamaHealthy: true });
    vi.mocked(stopLlamaServer).mockResolvedValue(undefined);
    const { result } = renderHook(() => useStopLlamaServer());
    await act(async () => { await result.current.stop(); });
    expect(stopLlamaServer).toHaveBeenCalled();
    expect(useBackendStore.getState().llamaHealthy).toBe(false);
  });
});

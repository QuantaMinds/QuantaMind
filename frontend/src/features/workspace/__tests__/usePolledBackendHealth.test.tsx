import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { usePolledBackendHealth } from "../hooks/useBackendHealth";
import { useBackendStore } from "../../../shared/state/backendStore";

beforeEach(() => {
  useBackendStore.setState({ selectedBackend: "llama_cpp" });
});

describe("usePolledBackendHealth", () => {
  it("never probes a DISABLED backend and marks it down (the remote-unconfigured gate)", async () => {
    const probe = vi.fn().mockResolvedValue({ available: true });
    const set = vi.fn();
    renderHook(() => usePolledBackendHealth("vllm", probe, set, { enabled: false }));
    await waitFor(() => expect(set).toHaveBeenCalledWith(false));
    expect(probe).not.toHaveBeenCalled();
  });

  it("probes an enabled backend and reports availability", async () => {
    const probe = vi.fn().mockResolvedValue({ available: true });
    const set = vi.fn();
    renderHook(() => usePolledBackendHealth("llama_cpp", probe, set, { enabled: true }));
    await waitFor(() => expect(set).toHaveBeenCalledWith(true));
  });

  it("fires onBecameAvailable once on the down→up edge", async () => {
    const probe = vi.fn().mockResolvedValue({ available: true });
    const onEdge = vi.fn();
    renderHook(() => usePolledBackendHealth("llama_cpp", probe, vi.fn(), { enabled: true, onBecameAvailable: onEdge }));
    await waitFor(() => expect(onEdge).toHaveBeenCalledTimes(1));
  });

  it("polls the SELECTED backend at 5s but a background backend only at 30s", async () => {
    vi.useFakeTimers();
    try {
      // selectedBackend is llama_cpp → llama polls FAST, mlx (background) polls SLOW.
      const fast = vi.fn().mockResolvedValue({ available: true });
      const slow = vi.fn().mockResolvedValue({ available: true });
      renderHook(() => usePolledBackendHealth("llama_cpp", fast, vi.fn(), { enabled: true }));
      renderHook(() => usePolledBackendHealth("mlx", slow, vi.fn(), { enabled: true }));
      await vi.advanceTimersByTimeAsync(0); // initial tick for both
      const fast0 = fast.mock.calls.length;
      const slow0 = slow.mock.calls.length;

      await vi.advanceTimersByTimeAsync(5000); // +5s
      expect(fast.mock.calls.length).toBeGreaterThan(fast0); // selected re-polled at 5s
      expect(slow.mock.calls.length).toBe(slow0); // background NOT polled at 5s

      await vi.advanceTimersByTimeAsync(25000); // total 30s
      expect(slow.mock.calls.length).toBeGreaterThan(slow0); // background polled at 30s
    } finally {
      vi.useRealTimers();
    }
  });
});

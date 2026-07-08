import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";

vi.mock("../../../shared/ipc/core/client", () => ({ checkLlamaHealth: vi.fn() }));

import { useLlamaBackend } from "../hooks/useBackendHealth";
import { checkLlamaHealth } from "../../../shared/ipc/core/client";
import { useBackendStore } from "../../../shared/state/backendStore";

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ llamaHealthy: null });
});

describe("useLlamaBackend", () => {
  it("polls llama.cpp health into the store on mount", async () => {
    vi.mocked(checkLlamaHealth).mockResolvedValue({ available: true, version: null });
    renderHook(() => useLlamaBackend());
    await waitFor(() => expect(useBackendStore.getState().llamaHealthy).toBe(true));
  });

  it("marks the backend down when the probe throws (server gone)", async () => {
    vi.mocked(checkLlamaHealth).mockRejectedValue(new Error("connection refused"));
    renderHook(() => useLlamaBackend());
    await waitFor(() => expect(useBackendStore.getState().llamaHealthy).toBe(false));
  });
});

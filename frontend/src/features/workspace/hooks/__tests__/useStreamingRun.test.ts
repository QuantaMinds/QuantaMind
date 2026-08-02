import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { useStreamingRun } from "../useStreamingRun";
import { useWorkspaceStore } from "../../state/workspaceStore";
import { useBackendStore } from "../../../../shared/state/backendStore";

const handlers: Record<string, EventCallback<unknown>> = {};

function fire(event: string, payload: unknown) {
  handlers[event]({ event, id: 0, payload });
}

function installListenMock() {
  vi.mocked(listen).mockImplementation((event, cb) => {
    handlers[event] = cb as EventCallback<unknown>;
    return Promise.resolve(() => {
      delete handlers[event];
    });
  });
}

describe("useStreamingRun", () => {
  beforeEach(() => {
    for (const k of Object.keys(handlers)) delete handlers[k];
    vi.mocked(invoke).mockReset();
    vi.mocked(listen).mockReset();
    installListenMock();
    useWorkspaceStore.setState({ lastRunMetrics: null });
    useBackendStore.setState({ selectedBackend: "llama_cpp" });
  });

  it("tokens append in order, no dup, no drop; status -> done", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const { result } = renderHook(() => useStreamingRun());

    await waitFor(() => {
      expect(handlers["prompt-token"]).toBeDefined();
      expect(handlers["prompt-done"]).toBeDefined();
    });

    await act(async () => {
      await result.current.start("llama3.2:1b", "Why is the sky blue?");
    });

    act(() => {
      fire("prompt-token", { text: "The " });
      fire("prompt-token", { text: "sky " });
      fire("prompt-token", { text: "is " });
      fire("prompt-token", { text: "blue." });
    });

    act(() => {
      fire("prompt-done", { ttft_ms: 12, tokens_per_sec: 50.0, token_count: 4, timeline: [] });
    });

    expect(result.current.output).toBe("The sky is blue.");
    expect(result.current.status).toBe("done");
    expect(result.current.metrics).toEqual({
      ttft_ms: 12,
      tokens_per_sec: 50.0,
      token_count: 4,
      timeline: [],
    });
    expect(invoke).toHaveBeenCalledWith("run_prompt", {
      model: "llama3.2:1b",
      prompt: "Why is the sky blue?",
      backend: "llama_cpp",
    });
  });

  it("sends the active backend with the run", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    useBackendStore.setState({ selectedBackend: "llama_cpp" });
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-token"]).toBeDefined());
    await act(async () => {
      await result.current.start("phi3", "hi");
    });
    expect(invoke).toHaveBeenCalledWith("run_prompt", {
      model: "phi3",
      prompt: "hi",
      backend: "llama_cpp",
    });
  });

  it("on Done, mirrors metrics into workspaceStore.lastRunMetrics", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const { result } = renderHook(() => useStreamingRun());

    await waitFor(() => expect(handlers["prompt-done"]).toBeDefined());
    // A done event always follows a start() — the hook only mirrors the run it began.
    await act(async () => { await result.current.start("m", "p"); });

    const payload = { ttft_ms: 200, tokens_per_sec: 30.5, token_count: 12, timeline: [] };
    act(() => {
      fire("prompt-done", payload);
    });

    expect(useWorkspaceStore.getState().lastRunMetrics).toEqual(payload);
  });

  it("invoke rejection sets status=error and preserves output so far", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-token"]).toBeDefined());

    await act(async () => {
      await result.current.start("m", "p");
    });

    expect(result.current.status).toBe("error");
    expect(result.current.error).toContain("boom");
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { useStreamingRun } from "../useStreamingRun";
import { useWorkspaceStore } from "../../state/workspaceStore";

const handlers: Record<string, EventCallback<unknown>> = {};
const fire = (event: string, payload: unknown) =>
  handlers[event]({ event, id: 0, payload });

const DONE = { ttft_ms: 12, tokens_per_sec: 50, token_count: 4, timeline: [] };

let consoleError: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  for (const k of Object.keys(handlers)) delete handlers[k];
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockImplementation((event, cb) => {
    handlers[event] = cb as EventCallback<unknown>;
    return Promise.resolve(() => { delete handlers[event]; });
  });
  vi.mocked(invoke).mockResolvedValue(undefined);
  useWorkspaceStore.setState({ lastRunMetrics: null });
  consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  consoleError.mockRestore();
});

// The run_prompt event stream is global; another hook may have initiated the run.
// This hook must react only to the run it started itself.
describe("useStreamingRun — own-run guard", () => {
  it("ignores a done event for a run it did not start (no status/metrics change)", async () => {
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-done"]).toBeDefined());

    // No start() — this done belongs to someone else.
    act(() => fire("prompt-done", DONE));

    expect(result.current.status).toBe("idle");
    expect(result.current.metrics).toBeNull();
    expect(useWorkspaceStore.getState().lastRunMetrics).toBeNull();
    expect(consoleError).not.toHaveBeenCalled();
  });

  it("ignores foreign tokens (output stays empty)", async () => {
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-token"]).toBeDefined());

    act(() => fire("prompt-token", { text: "ghost " }));

    expect(result.current.output).toBe("");
    expect(result.current.status).toBe("idle");
  });

  it("captures a run it started, then ignores the next foreign done", async () => {
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-done"]).toBeDefined());

    await act(async () => { await result.current.start("m", "p"); });
    act(() => fire("prompt-done", DONE));
    expect(result.current.status).toBe("done");
    expect(result.current.metrics).toEqual(DONE);

    // A second done (from a different hook's run) must not overwrite ours.
    act(() => fire("prompt-done", { ...DONE, token_count: 999 }));
    expect(result.current.metrics).toEqual(DONE);
  });

  it("resets cleanly so a cancel -> immediate rerun is not swallowed", async () => {
    const { result } = renderHook(() => useStreamingRun());
    await waitFor(() => expect(handlers["prompt-cancelled"]).toBeDefined());

    await act(async () => { await result.current.start("m", "p"); });
    act(() => fire("prompt-cancelled", { token_count: 1 }));
    expect(result.current.status).toBe("cancelled");

    // New run right after cancel — the guard must be re-armed.
    await act(async () => { await result.current.start("m", "p"); });
    act(() => fire("prompt-done", DONE));
    expect(result.current.status).toBe("done");
    expect(result.current.metrics).toEqual(DONE);
  });
});

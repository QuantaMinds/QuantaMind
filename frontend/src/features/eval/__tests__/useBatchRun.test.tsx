import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

// Capture the event handlers so a test can fire a malformed payload at them.
const { handlers } = vi.hoisted(() => ({ handlers: {} as Record<string, (e: { payload: unknown }) => void> }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, cb: (e: { payload: unknown }) => void) => {
    handlers[event] = cb;
    return Promise.resolve(() => {});
  }),
}));
// Keep the REAL schemas + event names (so the drift test exercises actual parsing);
// only stub the two IPC calls.
vi.mock("../../../shared/ipc/eval/batch", async (orig) => ({
  ...(await orig<typeof import("../../../shared/ipc/eval/batch")>()),
  runBatchEval: vi.fn().mockResolvedValue(undefined),
  stopBatchEval: vi.fn(),
}));
vi.mock("../../../shared/ipc/core/client", () => ({ healthFor: vi.fn(), credentialFor: vi.fn() }));

import { useBatchRun } from "../hooks/useBatchRun";
import { runBatchEval, stopBatchEval } from "../../../shared/ipc/eval/batch";
import { healthFor, credentialFor, type RemoteAuthReport } from "../../../shared/ipc/core/client";
import { useBatchStore } from "../state/batchStore";
import type { ModelTarget } from "../../../shared/ipc/eval/matrix";

const tasks = [
  { id: "t", category: "single", prompt: "p", tools: [{ name: "w", description: "", parameters: { type: "object", properties: {} } }], expected: { type: "call", name: "w", args: {} } },
] as never;

beforeEach(() => {
  vi.clearAllMocks();
  useBatchStore.getState().reset();
});

describe("useBatchRun pre-flight health check", () => {
  it("aborts with a clear message and never calls runBatchEval when the backend is down", async () => {
    vi.mocked(healthFor).mockResolvedValue({ available: false, version: null });
    const { result } = renderHook(() => useBatchRun());
    const targets: ModelTarget[] = [{ model: "llama3.2:1b", backend: "llama_cpp" }];

    await act(async () => { await result.current.run("c", targets, tasks, 1, 8, false); });

    expect(healthFor).toHaveBeenCalledWith("llama_cpp");
    expect(runBatchEval).not.toHaveBeenCalled();
    expect(useBatchStore.getState().error).toMatch(/llama\.cpp server isn't reachable/i);
    expect(useBatchStore.getState().running).toBe(false);
  });

  it("checks EVERY unique backend in a mixed run and aborts on the down one", async () => {
    // Ollama up, llama.cpp down → must still abort (not just check targets[0]).
    vi.mocked(healthFor).mockImplementation((b) =>
      Promise.resolve({ available: b === "ollama", version: null }),
    );
    const { result } = renderHook(() => useBatchRun());
    const targets: ModelTarget[] = [
      { model: "a", backend: "ollama" },
      { model: "b", backend: "ollama" },
      { model: "c", backend: "llama_cpp" },
    ];

    await act(async () => { await result.current.run("c", targets, tasks, 1, 8, false); });

    expect(vi.mocked(healthFor).mock.calls.map((c) => c[0])).toContain("llama_cpp");
    expect(runBatchEval).not.toHaveBeenCalled();
    expect(useBatchStore.getState().error).toMatch(/llama\.cpp server isn't reachable/i);
  });

  it("proceeds to runBatchEval when every backend is reachable", async () => {
    vi.mocked(healthFor).mockResolvedValue({ available: true, version: null });
    const { result } = renderHook(() => useBatchRun());
    const targets: ModelTarget[] = [{ model: "a", backend: "ollama" }];

    await act(async () => { await result.current.run("c", targets, tasks, 1, 8, false); });

    await waitFor(() => expect(runBatchEval).toHaveBeenCalled());
    expect(useBatchStore.getState().error).toBeNull();
  });

  // ── Remote-credential pre-flight (the seam: probe → real message → setError → abort) ──
  const authReport = (p: Partial<RemoteAuthReport>): RemoteAuthReport =>
    ({ status: "ok", http_status: null, host: "https://gpu.example.com:8000", insecure_key: false, ...p });

  it("blocks a vLLM run on a REJECTED KEY with the key message — never 'unreachable', never runs", async () => {
    vi.mocked(credentialFor).mockResolvedValue(authReport({ status: "unauthorized", http_status: 401 }));
    const { result } = renderHook(() => useBatchRun());
    const targets: ModelTarget[] = [{ model: "m", backend: "vllm" }];

    await act(async () => { await result.current.run("c", targets, tasks, 1, 8, false); });

    expect(credentialFor).toHaveBeenCalledWith("vllm");
    expect(healthFor).not.toHaveBeenCalled(); // remote routes through the credential probe, not healthFor
    expect(runBatchEval).not.toHaveBeenCalled();
    const err = useBatchStore.getState().error ?? "";
    expect(err).toMatch(/API key/i);
    expect(err).not.toMatch(/reach|start/i); // the never-conflate invariant, at the UI level
    expect(useBatchStore.getState().running).toBe(false);
  });

  it("blocks a DOWN vLLM server with the reach message — never blames the key", async () => {
    vi.mocked(credentialFor).mockResolvedValue(authReport({ status: "unreachable" }));
    const { result } = renderHook(() => useBatchRun());
    await act(async () => { await result.current.run("c", [{ model: "m", backend: "vllm" }], tasks, 1, 8, false); });
    const err = useBatchStore.getState().error ?? "";
    expect(err).toMatch(/reach/i);
    expect(err).not.toMatch(/API key/i);
    expect(runBatchEval).not.toHaveBeenCalled();
  });

  it("proceeds when the vLLM credential resolves OK", async () => {
    vi.mocked(credentialFor).mockResolvedValue(authReport({ status: "ok" }));
    const { result } = renderHook(() => useBatchRun());
    await act(async () => { await result.current.run("c", [{ model: "m", backend: "vllm" }], tasks, 1, 8, false); });
    await waitFor(() => expect(runBatchEval).toHaveBeenCalled());
    expect(useBatchStore.getState().error).toBeNull();
  });

  it("logs IPC payload drift instead of silently dropping a malformed batch event", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    renderHook(() => useBatchRun());
    await waitFor(() => expect(handlers["batch-progress"]).toBeDefined());

    // A payload that doesn't match BatchProgressSchema must NOT vanish — it logs.
    act(() => handlers["batch-progress"]({ payload: { totally: "wrong" } }));
    expect(spy).toHaveBeenCalledWith(
      expect.stringContaining("IPC payload drift (batch-progress)"),
      expect.anything(),
      expect.anything(),
    );
    spy.mockRestore();
  });
});

describe("useBatchRun stop", () => {
  it("flips the store's `stopping` flag synchronously, before stopBatchEval resolves", async () => {
    // A pending promise the test controls — proves the flag flips BEFORE the IPC
    // round-trip finishes, not after (the whole point of the fix: no wait for feedback).
    let resolveStop = () => {};
    vi.mocked(stopBatchEval).mockImplementation(() => new Promise<void>((res) => { resolveStop = res; }));
    useBatchStore.getState().startRun();
    const { result } = renderHook(() => useBatchRun());

    let stopPromise!: Promise<void>;
    act(() => {
      stopPromise = result.current.stop();
    });
    expect(useBatchStore.getState().stopping).toBe(true);
    expect(useBatchStore.getState().running).toBe(true); // still running — only stopping changed

    resolveStop();
    await act(async () => {
      await stopPromise;
    });
  });
});

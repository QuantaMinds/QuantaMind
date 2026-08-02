import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { listen, type EventCallback } from "@tauri-apps/api/event";
import {
  startDownloadEventBus,
  __resetDownloadEventBusForTests,
} from "../downloadEventBus";
import { useModelStore } from "../modelStore";

const handlers: Record<string, EventCallback<unknown>> = {};
const fire = (event: string, payload: unknown) =>
  handlers[event]({ event, id: 0, payload });

beforeEach(() => {
  for (const k of Object.keys(handlers)) delete handlers[k];
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockImplementation((event, cb) => {
    handlers[event] = cb as EventCallback<unknown>;
    return Promise.resolve(() => { delete handlers[event]; });
  });
  __resetDownloadEventBusForTests();
  useModelStore.setState({
    downloads: {}, pullNames: {}, activeHfName: null,
    activeTab: "ollama", pendingLocalPath: null,
  });
});

describe("downloadEventBus", () => {
  it("HF progress writes to activeHfName entry regardless of which component is mounted", async () => {
    await startDownloadEventBus();
    useModelStore.getState().setActiveHfName("llama-3.2");
    fire("hf-progress", { phase: "downloading", bytes_completed: 500, bytes_total: 1000, speed_bps: 100 });
    expect(useModelStore.getState().downloads["llama-3.2"]).toMatchObject({
      source: "huggingface", status: "downloading", percent: 50,
      bytesCompleted: 500, bytesTotal: 1000,
    });
  });

  it("Pull progress finds the model name via pullNames map (survives component unmounts)", async () => {
    await startDownloadEventBus();
    useModelStore.getState().recordPullName("pid-1", "phi3.5:latest");
    fire("pull-progress", {
      pull_id: "pid-1",
      name: "phi3.5:latest",
      progress: { phase: "downloading", digest: "sha", total: 1000, completed: 750, speed_bps: 100 },
    });
    expect(useModelStore.getState().downloads["phi3.5:latest"]).toMatchObject({
      source: "ollama", status: "downloading", percent: 75, pullId: "pid-1",
    });
  });

  it("Pull-progress with an unknown pullId still routes via the payload name (race-safe)", async () => {
    await startDownloadEventBus();
    fire("pull-progress", {
      pull_id: "unknown", name: "ghost",
      progress: { phase: "failed", message: "Ollama is not running. Start Ollama and try again." },
    });
    expect(useModelStore.getState().downloads["ghost"]).toMatchObject({
      source: "ollama", status: "error",
      error: "Ollama is not running. Start Ollama and try again.",
    });
  });

  it("startDownloadEventBus is idempotent — second call returns the same promise without re-attaching", async () => {
    const first = startDownloadEventBus();
    const second = startDownloadEventBus();
    expect(first).toBe(second);
    await first;
    // listen is called exactly 3 times (HF, pull, local), not 6.
    expect(vi.mocked(listen)).toHaveBeenCalledTimes(3);
  });

  it("startDownloadEventBus retries after a transient listen() rejection", async () => {
    // First call: listen rejects → bus singleton resets.
    vi.mocked(listen).mockRejectedValueOnce(new Error("tauri not ready"));
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(startDownloadEventBus()).rejects.toThrow("tauri not ready");
    await new Promise((r) => setTimeout(r, 0)); // let the .catch() reset starting
    // Second call: listen resolves → fresh subscription completes.
    vi.mocked(listen).mockImplementation((event, cb) => {
      handlers[event] = cb as EventCallback<unknown>;
      return Promise.resolve(() => { delete handlers[event]; });
    });
    await startDownloadEventBus();
    expect(handlers["pull-progress"]).toBeDefined();
    consoleSpy.mockRestore();
  });

  it("Pull-progress success phase flips status to success at 100%", async () => {
    await startDownloadEventBus();
    useModelStore.getState().recordPullName("pid-9", "qwen2.5:7b");
    fire("pull-progress", { pull_id: "pid-9", name: "qwen2.5:7b", progress: { phase: "success" } });
    expect(useModelStore.getState().downloads["qwen2.5:7b"]).toMatchObject({
      status: "success", percent: 100,
    });
  });
});

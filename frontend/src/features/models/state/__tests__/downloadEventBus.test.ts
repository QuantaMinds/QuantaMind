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
    downloads: {}, activeHfName: null,
    activeTab: "huggingface", pendingLocalPath: null,
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

  it("startDownloadEventBus is idempotent — second call returns the same promise without re-attaching", async () => {
    const first = startDownloadEventBus();
    const second = startDownloadEventBus();
    expect(first).toBe(second);
    await first;
    // listen is called exactly once (HF), not twice.
    expect(vi.mocked(listen)).toHaveBeenCalledTimes(1);
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
    expect(handlers["hf-progress"]).toBeDefined();
    consoleSpy.mockRestore();
  });

});

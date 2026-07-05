import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../../../../shared/ipc/models/storage", () => ({
  getInstalledModelsWithStats: vi.fn(),
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn(() => Promise.resolve([])),
}));

import { listen, type EventCallback } from "@tauri-apps/api/event";
import { getInstalledModelsWithStats } from "../../../../shared/ipc/models/storage";
import {
  startInstalledModelsBus,
  __resetInstalledModelsBusForTests,
} from "../installedModelsBus";
import { useInstalledModelsStore } from "../installedModelsStore";

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
  vi.mocked(getInstalledModelsWithStats).mockReset();
  vi.mocked(getInstalledModelsWithStats).mockResolvedValue([]);
  __resetInstalledModelsBusForTests();
  useInstalledModelsStore.setState({
    list: [], status: "idle", error: null, lastRefreshedAt: null,
  });
});

describe("installedModelsBus", () => {
  it("calls refresh() once on startup", async () => {
    await startInstalledModelsBus();
    // refresh() fans out to both backends via allSettled — let it settle.
    await new Promise((r) => setTimeout(r, 0));
    expect(getInstalledModelsWithStats).toHaveBeenCalledTimes(1);
    expect(useInstalledModelsStore.getState().status).toBe("ready");
  });

  it("refreshes again when models-changed fires", async () => {
    await startInstalledModelsBus();
    // Wait for the first refresh to settle so the second isn't coalesced.
    await new Promise((r) => setTimeout(r, 0));
    vi.mocked(getInstalledModelsWithStats).mockClear();
    fire("models-changed", null);
    await new Promise((r) => setTimeout(r, 0));
    expect(getInstalledModelsWithStats).toHaveBeenCalledTimes(1);
  });

  it("is idempotent — second call returns the same promise, no re-attach", async () => {
    const a = startInstalledModelsBus();
    const b = startInstalledModelsBus();
    expect(a).toBe(b);
    await a;
    expect(vi.mocked(listen)).toHaveBeenCalledTimes(1);
  });

  it("resets the singleton if listen() rejects so a later call can retry", async () => {
    vi.mocked(listen).mockRejectedValueOnce(new Error("tauri not ready"));
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(startInstalledModelsBus()).rejects.toThrow("tauri not ready");
    await new Promise((r) => setTimeout(r, 0));
    vi.mocked(listen).mockImplementation((event, cb) => {
      handlers[event] = cb as EventCallback<unknown>;
      return Promise.resolve(() => { delete handlers[event]; });
    });
    await startInstalledModelsBus();
    expect(handlers["models-changed"]).toBeDefined();
    spy.mockRestore();
  });
});

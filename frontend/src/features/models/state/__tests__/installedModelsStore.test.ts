import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../../shared/ipc/models/storage", () => ({
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn(),
}));

import { listVllmModels, listSglangModels } from "../../../../shared/ipc/models/storage";
import { listLlamaModels } from "../../../../shared/ipc/models/llama_start";
import { useInstalledModelsStore } from "../installedModelsStore";

const fake = (name: string, backend: "llama_cpp" | "vllm" | "sglang" = "llama_cpp") => ({
  name,
  size_bytes: 0,
  modified_at: "2025-01-01T00:00:00Z",
  family: "x",
  parameter_size: "1B",
  quantization: "Q4",
  backend,
});

beforeEach(() => {
  vi.mocked(listLlamaModels).mockReset().mockResolvedValue([]);
  vi.mocked(listVllmModels).mockReset().mockResolvedValue([]);
  vi.mocked(listSglangModels).mockReset().mockResolvedValue([]);
  useInstalledModelsStore.setState({
    list: [],
    status: "idle",
    error: null,
    lastRefreshedAt: null,
  });
});

describe("installedModelsStore", () => {
  it("refresh() populates list and flips status to ready", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([fake("a"), fake("b")]);
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.list.map((m) => m.name)).toEqual(["a", "b"]);
    expect(s.error).toBeNull();
  });

  it("merges the local list with each reachable remote server's models", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([fake("local")]);
    vi.mocked(listVllmModels).mockResolvedValue([fake("on-vllm", "vllm")]);
    vi.mocked(listSglangModels).mockResolvedValue([fake("on-sglang", "sglang")]);
    await useInstalledModelsStore.getState().refresh();
    expect(useInstalledModelsStore.getState().list.map((m) => m.name)).toEqual([
      "local",
      "on-vllm",
      "on-sglang",
    ]);
  });

  it("an unconfigured/unreachable remote is not an error — the local list still loads", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([fake("local")]);
    vi.mocked(listVllmModels).mockRejectedValue(new Error("not configured"));
    vi.mocked(listSglangModels).mockRejectedValue(new Error("unreachable"));
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.error).toBeNull();
    expect(s.list.map((m) => m.name)).toEqual(["local"]);
  });

  it("surfaces an error when the LOCAL source fails", async () => {
    vi.mocked(listLlamaModels).mockRejectedValue(new Error("boom"));
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("error");
    expect(s.error).toMatch(/boom/);
  });

  it("de-dupes concurrent refreshes — a second call while loading is a no-op", async () => {
    let resolve: (v: never[]) => void = () => {};
    vi.mocked(listLlamaModels).mockReturnValue(new Promise((r) => { resolve = r; }));
    const first = useInstalledModelsStore.getState().refresh();
    void useInstalledModelsStore.getState().refresh();
    resolve([]);
    await first;
    expect(listLlamaModels).toHaveBeenCalledTimes(1);
  });
});

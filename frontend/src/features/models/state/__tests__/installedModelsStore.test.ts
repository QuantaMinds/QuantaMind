import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("../../../../shared/ipc/models/storage", () => ({
  getInstalledModelsWithStats: vi.fn(),
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn(),
}));
vi.mock("../../../../shared/ipc/models/mlx", () => ({
  listMlxModels: vi.fn(),
}));

import { getInstalledModelsWithStats } from "../../../../shared/ipc/models/storage";
import { listLlamaModels } from "../../../../shared/ipc/models/llama_start";
import { listMlxModels } from "../../../../shared/ipc/models/mlx";
import { useInstalledModelsStore } from "../installedModelsStore";

const fake = (name: string, backend: "ollama" | "llama_cpp" | "mlx" = "ollama") => ({
  name,
  size_bytes: 0,
  modified_at: "2025-01-01T00:00:00Z",
  family: "x",
  parameter_size: "1B",
  quantization: "Q4",
  backend,
});

beforeEach(() => {
  vi.mocked(getInstalledModelsWithStats).mockReset();
  vi.mocked(listLlamaModels).mockReset().mockResolvedValue([]);
  vi.mocked(listMlxModels).mockReset().mockResolvedValue([]);
  useInstalledModelsStore.setState({
    list: [],
    status: "idle",
    error: null,
    lastRefreshedAt: null,
  });
});

describe("installedModelsStore", () => {
  it("refresh() populates list and flips status to ready", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([fake("a"), fake("b")]);
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.list.map((m) => m.name)).toEqual(["a", "b"]);
    expect(s.error).toBeNull();
    expect(s.lastRefreshedAt).not.toBeNull();
  });

  it("refresh() merges Ollama and llama.cpp models", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([fake("llama3", "ollama")]);
    vi.mocked(listLlamaModels).mockResolvedValue([fake("phi3", "llama_cpp")]);
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.list.map((m) => m.name)).toEqual(["llama3", "phi3"]);
  });

  it("refresh() merges MLX models alongside the other backends", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([fake("llama3", "ollama")]);
    vi.mocked(listMlxModels).mockResolvedValue([fake("mlx-community/x-4bit", "mlx")]);
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.list.map((m) => m.name)).toEqual(["llama3", "mlx-community/x-4bit"]);
    expect(s.list.find((m) => m.name === "mlx-community/x-4bit")?.backend).toBe("mlx");
  });

  it("refresh() still lists llama.cpp models when Ollama is down", async () => {
    vi.mocked(getInstalledModelsWithStats).mockRejectedValue(new Error("Ollama down"));
    vi.mocked(listLlamaModels).mockResolvedValue([fake("phi3", "llama_cpp")]);
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("ready");
    expect(s.list.map((m) => m.name)).toEqual(["phi3"]);
  });

  it("refresh() records an error only when every backend fails", async () => {
    vi.mocked(getInstalledModelsWithStats).mockRejectedValue(new Error("boom"));
    vi.mocked(listLlamaModels).mockRejectedValue(new Error("no llama"));
    vi.mocked(listMlxModels).mockRejectedValue(new Error("no mlx"));
    await useInstalledModelsStore.getState().refresh();
    const s = useInstalledModelsStore.getState();
    expect(s.status).toBe("error");
    expect(s.error).toContain("boom");
  });

  it("concurrent refreshes coalesce — IPC called once for two parallel refresh() calls", async () => {
    let resolveIt: (v: ReturnType<typeof fake>[]) => void = () => {};
    vi.mocked(getInstalledModelsWithStats).mockReturnValue(
      new Promise((resolve) => { resolveIt = resolve; }),
    );
    const p1 = useInstalledModelsStore.getState().refresh();
    const p2 = useInstalledModelsStore.getState().refresh();
    resolveIt([fake("only")]);
    await Promise.all([p1, p2]);
    expect(getInstalledModelsWithStats).toHaveBeenCalledTimes(1);
    expect(useInstalledModelsStore.getState().list.map((m) => m.name)).toEqual(["only"]);
  });
});

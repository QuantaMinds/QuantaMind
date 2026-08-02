import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../storage/StorageSection", () => ({ StorageSection: () => null }));
vi.mock("../../../../../shared/ipc/models/storage", () => ({
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn(),
  deleteLlamaModel: vi.fn(),
}));

import { listLlamaModels } from "../../../../../shared/ipc/models/llama_start";
import { DownloadsTab } from "../DownloadsTab";
import { useModelStore } from "../../../state/modelStore";
import { useInstalledModelsStore } from "../../../state/installedModelsStore";

beforeEach(() => {
  useModelStore.setState({
    downloads: {},
    pendingLocalPath: null,
  });
  vi.mocked(listLlamaModels).mockReset();
  useInstalledModelsStore.setState({
    list: [], status: "idle", error: null, lastRefreshedAt: null,
  });
});

describe("DownloadsTab", () => {
  it("shows empty copy when nothing is in-progress and nothing installed", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([]);
    render(<DownloadsTab />);
    expect(await screen.findByTestId("downloads-empty-active")).toBeInTheDocument();
    expect(await screen.findByTestId("downloads-empty-installed")).toBeInTheDocument();
  });

  it("renders in-progress entry with progress bar and a Cancel button", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([]);
    useModelStore.getState().upsertDownload({
      id: "qwen2.5:7b", source: "huggingface", name: "qwen2.5:7b",
      status: "downloading", percent: 37, bytesCompleted: 100, bytesTotal: 271,
    });
    render(<DownloadsTab />);
    expect(await screen.findByTestId("download-active-qwen2.5:7b")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /cancel qwen2.5:7b/i })).toBeInTheDocument();
  });

  it("renders installed models with a Delete button per item", async () => {
    vi.mocked(listLlamaModels).mockResolvedValue([
      {
        name: "phi3.5", path: "/g/phi3.5.gguf",
        family: "phi3", parameter_size: "3.8B", quantization: "Q4_K_M",
        size_bytes: 2_400_000_000, modified_at: "2026-05-22", backend: "llama_cpp" as const,
      },
    ]);
    render(<DownloadsTab />);
    expect(await screen.findByTestId("download-installed-phi3.5")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /delete phi3\.5/i })).toBeInTheDocument();
  });
});

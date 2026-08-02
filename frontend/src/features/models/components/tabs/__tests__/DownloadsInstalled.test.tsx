import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../../../shared/ipc/models/storage", () => ({
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn().mockResolvedValue([]),
  deleteLlamaModel: vi.fn(),
}));
vi.mock("../../../../../shared/ui/Toast", () => ({ useToast: () => vi.fn() }));

import { DownloadsInstalled } from "../DownloadsInstalled";
import { useInstalledModelsStore } from "../../../state/installedModelsStore";

const meta = { family: "phi", parameter_size: "3.8B", quantization: "Q4_K_M", modified_at: "", size_bytes: 100 };
const local = { ...meta, name: "phi-4-mini", backend: "llama_cpp" as const, path: "/g/phi-4-mini.gguf" };
const remote = { ...meta, name: "Qwen/Qwen2.5-7B", backend: "vllm" as const };
const set = (list: unknown[]) => useInstalledModelsStore.setState({ list: list as never, status: "ready" });

beforeEach(() => {
  useInstalledModelsStore.setState({ list: [], status: "ready", error: null, lastRefreshedAt: null });
});

describe("DownloadsInstalled rendering", () => {
  it("shows the empty state when nothing is installed", () => {
    render(<DownloadsInstalled />);
    expect(screen.getByTestId("downloads-empty-installed")).toBeInTheDocument();
  });

  it("badges a local model llama.cpp and offers Delete", () => {
    set([local]);
    render(<DownloadsInstalled />);
    const row = screen.getByTestId("download-installed-phi-4-mini");
    expect(row).toHaveTextContent("llama.cpp");
    expect(screen.getByRole("button", { name: /delete phi-4-mini/i })).toBeInTheDocument();
  });

  it("badges a remote-served model and offers no Delete — it isn't on this machine", () => {
    set([remote]);
    render(<DownloadsInstalled />);
    const row = screen.getByTestId("download-installed-Qwen/Qwen2.5-7B");
    expect(row).toHaveTextContent("remote");
    expect(screen.queryByRole("button", { name: /delete Qwen/i })).toBeNull();
  });

  it("collapses the same model served locally and remotely into one row", () => {
    set([local, { ...remote, name: "phi-4-mini" }]);
    render(<DownloadsInstalled />);
    expect(screen.getAllByTestId(/^download-installed-/)).toHaveLength(1);
  });
});

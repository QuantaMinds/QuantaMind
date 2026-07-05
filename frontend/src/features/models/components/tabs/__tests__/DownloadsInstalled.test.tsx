import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../../../shared/ipc/models/storage", () => ({
  getInstalledModelsWithStats: vi.fn(),
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
  removeModel: vi.fn(),
}));
vi.mock("../../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn().mockResolvedValue([]),
  deleteLlamaModel: vi.fn(),
}));
vi.mock("../../../../../shared/ipc/models/gguf", () => ({ installLocalGguf: vi.fn() }));
vi.mock("../../../../../shared/ui/Toast", () => ({ useToast: () => vi.fn() }));

import { installLocalGguf } from "../../../../../shared/ipc/models/gguf";
import { DownloadsInstalled } from "../DownloadsInstalled";
import { useInstalledModelsStore } from "../../../state/installedModelsStore";

const meta = { family: "phi", parameter_size: "3.8B", quantization: "Q4_K_M", modified_at: "", size_bytes: 100 };
const ollama = { ...meta, name: "phi3.5:latest", backend: "ollama" as const };
const llama = { ...meta, name: "phi-4-mini", backend: "llama_cpp" as const, path: "/g/phi-4-mini.gguf" };
const set = (list: unknown[]) => useInstalledModelsStore.setState({ list: list as never, status: "ready" });

beforeEach(() => {
  vi.mocked(installLocalGguf).mockReset().mockResolvedValue(undefined);
  useInstalledModelsStore.setState({ list: [], status: "ready", error: null, lastRefreshedAt: null });
});

describe("DownloadsInstalled rendering", () => {
  it("shows the empty state when nothing is installed", () => {
    render(<DownloadsInstalled />);
    expect(screen.getByTestId("downloads-empty-installed")).toBeInTheDocument();
  });

  it("badges an Ollama model and offers Delete", () => {
    set([ollama]);
    render(<DownloadsInstalled />);
    expect(screen.getByTestId("download-installed-phi3.5")).toHaveTextContent("Ollama");
    expect(screen.getByRole("button", { name: /delete phi3\.5/i })).toBeInTheDocument();
  });

  it("folder-only: shows llama.cpp badge + Add to Ollama and imports on click", async () => {
    set([llama]);
    render(<DownloadsInstalled />);
    expect(screen.getByTestId("download-installed-phi-4-mini")).toHaveTextContent("llama.cpp");
    fireEvent.click(screen.getByTestId("add-to-ollama-phi-4-mini"));
    await waitFor(() =>
      expect(installLocalGguf).toHaveBeenCalledWith("/g/phi-4-mini.gguf", "phi-4-mini"),
    );
  });

  it("collapses a both-backends model into one row with both badges, no Add button", () => {
    set([{ ...llama, name: "dup" }, { ...ollama, name: "dup:latest" }]);
    render(<DownloadsInstalled />);
    const row = screen.getByTestId("download-installed-dup");
    expect(row).toHaveTextContent("Ollama");
    expect(row).toHaveTextContent("llama.cpp");
    expect(screen.queryByTestId("add-to-ollama-dup")).toBeNull();
  });
});

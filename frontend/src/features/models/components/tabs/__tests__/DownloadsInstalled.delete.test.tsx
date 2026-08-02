import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../../../shared/ipc/models/storage", () => ({
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../../../../shared/ipc/models/llama_start", () => ({
  listLlamaModels: vi.fn().mockResolvedValue([]),
  deleteLlamaModel: vi.fn(),
}));
vi.mock("../../../../../shared/ipc/models/gguf", () => ({ installLocalGguf: vi.fn() }));
vi.mock("../../../../../shared/ui/Toast", () => ({ useToast: () => vi.fn() }));

import { deleteLlamaModel } from "../../../../../shared/ipc/models/llama_start";
import { DownloadsInstalled } from "../DownloadsInstalled";
import { useInstalledModelsStore } from "../../../state/installedModelsStore";

const meta = { family: "phi", parameter_size: "3.8B", quantization: "Q4_K_M", modified_at: "", size_bytes: 100 };
const local = { ...meta, name: "phi-4-mini", backend: "llama_cpp" as const, path: "/g/phi-4-mini.gguf" };
const remote = { ...meta, name: "served-elsewhere", backend: "vllm" as const };
const set = (list: unknown[]) => useInstalledModelsStore.setState({ list: list as never, status: "ready" });

beforeEach(() => {
  vi.mocked(deleteLlamaModel).mockReset().mockResolvedValue(undefined);
  useInstalledModelsStore.setState({ list: [], status: "ready", error: null, lastRefreshedAt: null });
});

describe("DownloadsInstalled delete", () => {
  it("Delete removes the local GGUF file by its path", async () => {
    set([local]);
    render(<DownloadsInstalled />);
    fireEvent.click(screen.getByRole("button", { name: /delete phi-4-mini/i }));
    fireEvent.click(screen.getByRole("button", { name: /^remove$/i }));
    await waitFor(() => expect(deleteLlamaModel).toHaveBeenCalledWith("/g/phi-4-mini.gguf"));
  });

  it("cancelling the confirm deletes nothing", async () => {
    set([local]);
    render(<DownloadsInstalled />);
    fireEvent.click(screen.getByRole("button", { name: /delete phi-4-mini/i }));
    fireEvent.click(screen.getByRole("button", { name: /^cancel$/i }));
    expect(screen.queryByTestId("downloads-confirm-delete")).toBeNull();
    expect(deleteLlamaModel).not.toHaveBeenCalled();
  });

  it("a remote-served model offers no Delete — it isn't ours to remove", () => {
    set([remote]);
    render(<DownloadsInstalled />);
    expect(screen.queryByRole("button", { name: /delete served-elsewhere/i })).toBeNull();
    expect(deleteLlamaModel).not.toHaveBeenCalled();
  });

  it("surfaces a delete failure instead of silently leaving the row", async () => {
    vi.mocked(deleteLlamaModel).mockRejectedValue(new Error("permission denied"));
    set([local]);
    render(<DownloadsInstalled />);
    fireEvent.click(screen.getByRole("button", { name: /delete phi-4-mini/i }));
    fireEvent.click(screen.getByRole("button", { name: /^remove$/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/permission denied/i);
  });
});

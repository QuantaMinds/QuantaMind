import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("../../../shared/ipc/models/storage", () => ({
  getInstalledModelsWithStats: vi.fn(),
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { getInstalledModelsWithStats } from "../../../shared/ipc/models/storage";
import { ModelPicker } from "../components/model-select/ModelPicker";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useNavStore } from "../../../shared/state/navStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";

const M = (name: string, family = "llama") => ({
  name, size_bytes: 1_000_000_000, modified_at: "", family,
  parameter_size: "", quantization: "", backend: "ollama" as const,
});

describe("ModelPicker", () => {
  beforeEach(() => {
    vi.mocked(getInstalledModelsWithStats).mockReset();
    useInstalledModelsStore.setState({
      list: [], status: "idle", error: null, lastRefreshedAt: null,
    });
    useBackendStore.setState({ ollamaHealthy: null });
  });

  it("renders each generative Ollama model name as an option", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([M("llama3.2:1b"), M("mistral:7b")]);
    render(<ModelPicker value={null} onChange={() => {}} />);
    expect(await screen.findByRole("option", { name: "llama3.2:1b" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "mistral:7b" })).toBeInTheDocument();
  });

  it("silently hides embedding models from the dropdown with no visible hint", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([
      M("llama3.2:1b"),
      M("nomic-embed-text:latest", "nomic-bert"),
      M("snowflake-arctic-embed:l", "bert"),
    ]);
    render(<ModelPicker value={null} onChange={() => {}} />);
    await screen.findByRole("option", { name: "llama3.2:1b" });
    expect(screen.queryByRole("option", { name: "nomic-embed-text:latest" })).toBeNull();
    expect(screen.queryByRole("option", { name: "snowflake-arctic-embed:l" })).toBeNull();
    expect(screen.queryByTestId("picker-hidden-count")).toBeNull();
  });

  it("fires onChange with the selected model name", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([M("llama3.2:1b"), M("mistral:7b")]);
    const onChange = vi.fn();
    render(<ModelPicker value={null} onChange={onChange} />);
    await screen.findByRole("option", { name: "mistral:7b" });
    const select = screen.getByRole("combobox", { name: /model/i }) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "mistral:7b" } });
    expect(onChange).toHaveBeenCalledWith("mistral:7b");
  });

  it("controlled value persists across rerenders", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([M("llama3.2:1b"), M("mistral:7b")]);
    const { rerender } = render(<ModelPicker value={null} onChange={() => {}} />);
    await screen.findByRole("option", { name: "mistral:7b" });
    rerender(<ModelPicker value="mistral:7b" onChange={() => {}} />);
    const select = screen.getByRole("combobox", { name: /model/i }) as HTMLSelectElement;
    expect(select.value).toBe("mistral:7b");
  });

  it("shows an error message when the IPC rejects", async () => {
    vi.mocked(getInstalledModelsWithStats).mockRejectedValue(new Error("HTTP 503"));
    render(<ModelPicker value={null} onChange={() => {}} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("HTTP 503");
  });

  it("shows friendly Ollama-down message when StatusBar marks health=false", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([M("llama3.2:1b")]);
    render(<ModelPicker value={null} onChange={() => {}} />);
    await screen.findByRole("option", { name: "llama3.2:1b" });
    useBackendStore.getState().setOllamaHealthy(false);
    expect(await screen.findByRole("alert")).toHaveTextContent(/Ollama is not running/);
  });

  it("refreshes the model list once when health flips from false to true", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([]);
    useBackendStore.setState({ ollamaHealthy: false });
    render(<ModelPicker value={null} onChange={() => {}} />);
    // Let the initial-idle refresh resolve before clearing the mock —
    // refresh() coalesces concurrent calls so the health flip would
    // otherwise be a no-op while the first call is still in flight.
    await waitFor(() => expect(useInstalledModelsStore.getState().status).toBe("ready"));
    vi.mocked(getInstalledModelsWithStats).mockClear();
    useBackendStore.getState().setOllamaHealthy(true);
    await waitFor(() => expect(getInstalledModelsWithStats).toHaveBeenCalledTimes(1));
  });

  it("Add Model button navigates the top tab to 'models' via navStore", async () => {
    vi.mocked(getInstalledModelsWithStats).mockResolvedValue([]);
    useNavStore.setState({ topView: "workspace" });
    render(<ModelPicker value={null} onChange={() => {}} />);
    fireEvent.click(screen.getByTestId("add-model-button"));
    expect(useNavStore.getState().topView).toBe("models");
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../../shared/ipc/models/storage", () => ({
  getInstalledModelsWithStats: vi.fn(),
  listVllmModels: vi.fn().mockResolvedValue([]),
  listSglangModels: vi.fn().mockResolvedValue([]),
}));

import { invoke } from "@tauri-apps/api/core";
import { getInstalledModelsWithStats } from "../../../shared/ipc/models/storage";
import { ModelPicker } from "../components/model-select/ModelPicker";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";

const M = (name: string) => ({
  name, size_bytes: 1, modified_at: "", family: "llama",
  parameter_size: "", quantization: "", backend: "ollama" as const,
});

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
  vi.mocked(getInstalledModelsWithStats).mockReset().mockResolvedValue([M("llama3.2:1b")]);
  useInstalledModelsStore.setState({
    list: [], status: "idle", error: null, lastRefreshedAt: null,
  });
  useBackendStore.setState({ ollamaHealthy: true });
});

describe("ModelPicker Stop button", () => {
  it("appears only when Ollama is healthy", async () => {
    const { rerender } = render(<ModelPicker value={null} onChange={() => {}} />);
    expect(await screen.findByTestId("ollama-stop-button")).toBeInTheDocument();
    useBackendStore.setState({ ollamaHealthy: false });
    rerender(<ModelPicker value={null} onChange={() => {}} />);
    expect(screen.queryByTestId("ollama-stop-button")).toBeNull();
  });

  it("clicking Stop invokes stop_ollama and flips ollamaHealthy to false", async () => {
    render(<ModelPicker value={null} onChange={() => {}} />);
    fireEvent.click(await screen.findByTestId("ollama-stop-button"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_ollama"));
    await waitFor(() =>
      expect(useBackendStore.getState().ollamaHealthy).toBe(false),
    );
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../features/workspace/hooks/useStartOllama", () => ({ useStartOllama: () => ({ start: vi.fn(), status: "idle" }) }));
vi.mock("../features/workspace/hooks/useStopOllama", () => ({ useStopOllama: () => ({ stop: vi.fn(), status: "idle" }) }));
vi.mock("../features/workspace/hooks/useStartLlamaServer", () => ({ useStartLlamaServer: () => ({ start: vi.fn(), status: "idle", error: null }) }));
vi.mock("../features/workspace/hooks/useStopLlamaServer", () => ({ useStopLlamaServer: () => ({ stop: vi.fn(), status: "idle" }) }));
vi.mock("../features/workspace/hooks/useMlxServer", () => ({ useMlxServer: () => ({ start: vi.fn(), stop: vi.fn(), starting: false, phase: null, error: null }) }));

import { invoke } from "@tauri-apps/api/core";
import { GlobalControls } from "../GlobalControls";
import { useBackendStore } from "../shared/state/backendStore";

function mockInvoke(apple: boolean) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_hardware_snapshot")
      return Promise.resolve({ total_memory_bytes: 16, available_memory_bytes: 8, is_apple_silicon: apple });
    if (cmd === "check_mlx_health") return Promise.resolve({ available: false, version: null });
    return Promise.resolve([]);
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  useBackendStore.setState({ selectedBackend: "ollama", ollamaHealthy: false, llamaHealthy: null, mlxHealthy: null });
});

describe("GlobalControls (global header)", () => {
  const backendSelect = () => screen.getByTestId("header-backend-select") as HTMLSelectElement;

  it("renders the backend selector and the active server control together", () => {
    mockInvoke(false);
    render(<GlobalControls />);
    expect(screen.getByTestId("header-backend-selector")).toBeInTheDocument();
    expect(backendSelect().value).toBe("ollama");
    // Ollama active + down → its Start control shows.
    expect(screen.getByTestId("ollama-start")).toBeInTheDocument();
  });

  it("choosing a backend switches the global selection and the server control", () => {
    mockInvoke(false);
    render(<GlobalControls />);
    fireEvent.change(backendSelect(), { target: { value: "llama_cpp" } });
    expect(useBackendStore.getState().selectedBackend).toBe("llama_cpp");
    expect(screen.getByTestId("llama-start")).toBeInTheDocument();
    expect(screen.queryByTestId("ollama-start")).toBeNull();
  });

  it("shows the MLX option only on Apple Silicon", async () => {
    mockInvoke(true);
    render(<GlobalControls />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_hardware_snapshot"));
    expect(backendSelect().querySelector('option[value="mlx"]')).not.toBeNull();
  });

  it("hides MLX off Apple Silicon", async () => {
    mockInvoke(false);
    render(<GlobalControls />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_hardware_snapshot"));
    expect(backendSelect().querySelector('option[value="mlx"]')).toBeNull();
  });
});

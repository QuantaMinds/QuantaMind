import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
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
  useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: null});
});

describe("GlobalControls (global header)", () => {
  const backendSelect = () => screen.getByTestId("header-backend-select") as HTMLSelectElement;

  it("renders the backend selector and the active server control together", () => {
    mockInvoke(false);
    render(<GlobalControls />);
    expect(screen.getByTestId("header-backend-selector")).toBeInTheDocument();
    expect(backendSelect().value).toBe("llama_cpp");
    // llama.cpp active + down → its Start control shows.
    expect(screen.getByTestId("llama-start")).toBeInTheDocument();
  });

  it("choosing a backend switches the global selection and the server control", () => {
    mockInvoke(false);
    render(<GlobalControls />);
    fireEvent.change(backendSelect(), { target: { value: "vllm" } });
    expect(useBackendStore.getState().selectedBackend).toBe("vllm");
    // A remote backend the app can't start → no local Start control.
    expect(screen.queryByTestId("llama-start")).toBeNull();
  });

});

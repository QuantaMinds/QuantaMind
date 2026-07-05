import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@monaco-editor/react", () => ({ default: () => <textarea data-testid="prompt-input" /> }));

import { Workspace } from "../Workspace";
import { useWorkspacesStore } from "../../../workspaces/state/workspaceStore";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";

beforeEach(() => {
  vi.clearAllMocks();
  // A healthy LLM so the run surface (not the setup guide) renders by default.
  useBackendStore.setState({ selectedBackend: "ollama", ollamaHealthy: true, llamaHealthy: null, mlxHealthy: null, vllmHealthy: null, sglangHealthy: null });
  useSelectedModelStore.setState({ selectedModels: [] });
  useWorkspacesStore.setState({
    root: "/ws", tree: [], currentPath: "/ws/a.quantamind.yaml",
    current: { name: "a", system: "", user: "hi", model: null, params: {}, created_at: "t", updated_at: "t", auto_rerun: false },
    dirty: false,
  });
});

describe("Workspace (adaptive run surface)", () => {
  it("one global model → the single-run surface, no compare strategy picker", () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "ollama", size_bytes: 1 }] });
    render(<Workspace />);
    expect(screen.getByTestId("run-status")).toBeTruthy();
    expect(screen.queryByTestId("run-strategy-picker")).toBeNull();
    expect(screen.queryByTestId("multi-toolbar")).toBeNull();
  });

  it("2+ Ollama models → the compare surface (strategy picker + multi run), no single-run", () => {
    useSelectedModelStore.setState({ selectedModels: [
      { name: "llama3.2:1b", backend: "ollama", size_bytes: 1 },
      { name: "mistral:7b", backend: "ollama", size_bytes: 1 },
    ] });
    render(<Workspace />);
    expect(screen.getByTestId("run-strategy-picker")).toBeTruthy();
    expect(screen.getByTestId("multi-toolbar")).toBeTruthy();
    expect(screen.queryByTestId("run-status")).toBeNull();
  });

  it("with no global model, Run is disabled and a pick-a-model hint shows", () => {
    render(<Workspace />);
    expect(screen.getByTestId("no-model-hint")).toBeInTheDocument();
    expect((screen.getByRole("button", { name: /^run$/i }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("no LLM running → the backend setup guide replaces the run surface", () => {
    useBackendStore.setState({ ollamaHealthy: false, llamaHealthy: false, mlxHealthy: false });
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "ollama", size_bytes: 1 }] });
    render(<Workspace />);
    expect(screen.getByTestId("backend-setup-guide")).toBeInTheDocument();
    expect(screen.getByTestId("setup-engine-ollama")).toBeInTheDocument();
    expect(screen.queryByTestId("run-status")).toBeNull();
  });

  it("a healthy remote backend (vLLM) also shows the run surface, not the setup guide", () => {
    useBackendStore.setState({
      selectedBackend: "vllm",
      ollamaHealthy: false,
      llamaHealthy: false,
      mlxHealthy: false,
      vllmHealthy: true,
    });
    useSelectedModelStore.setState({ selectedModels: [{ name: "Qwen/Qwen2.5-7B-Instruct-AWQ", backend: "vllm", size_bytes: 0 }] });
    render(<Workspace />);
    expect(screen.queryByTestId("backend-setup-guide")).toBeNull();
    expect(screen.getByTestId("run-status")).toBeInTheDocument();
  });

  it("a running LLM switches from the guide to the run surface", () => {
    useBackendStore.setState({ ollamaHealthy: false, llamaHealthy: false, mlxHealthy: false });
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "ollama", size_bytes: 1 }] });
    const { rerender } = render(<Workspace />);
    expect(screen.getByTestId("backend-setup-guide")).toBeInTheDocument();
    act(() => useBackendStore.setState({ ollamaHealthy: true }));
    rerender(<Workspace />);
    expect(screen.queryByTestId("backend-setup-guide")).toBeNull();
    expect(screen.getByTestId("run-status")).toBeInTheDocument();
  });

  it("MLX: Run is disabled until the MLX server is healthy", () => {
    useBackendStore.setState({ selectedBackend: "mlx", mlxHealthy: false });
    useSelectedModelStore.setState({ selectedModels: [{ name: "stub-mlx", backend: "mlx", size_bytes: 0 }] });
    const { rerender } = render(<Workspace />);
    const runBtn = () => screen.getByRole("button", { name: /^run$/i }) as HTMLButtonElement;
    expect(runBtn().disabled).toBe(true);
    act(() => useBackendStore.setState({ mlxHealthy: true }));
    rerender(<Workspace />);
    expect(runBtn().disabled).toBe(false);
  });
});

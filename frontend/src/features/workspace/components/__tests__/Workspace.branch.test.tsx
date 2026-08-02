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
  useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: null, vllmHealthy: null,});
  useSelectedModelStore.setState({ selectedModels: [] });
  useWorkspacesStore.setState({
    root: "/ws", tree: [], currentPath: "/ws/a.quantamind.yaml",
    current: { name: "a", system: "", user: "hi", model: null, params: {}, created_at: "t", updated_at: "t", auto_rerun: false },
    dirty: false,
  });
});

describe("Workspace (adaptive run surface)", () => {
  it("a selected model → the single-run surface", () => {
    useBackendStore.setState({ llamaHealthy: true });
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "llama_cpp", size_bytes: 1 }] });
    render(<Workspace />);
    expect(screen.getByTestId("run-status")).toBeTruthy();
  });

  it("with no global model, Run is disabled and a pick-a-model hint shows", () => {
    useBackendStore.setState({ llamaHealthy: true });
    render(<Workspace />);
    expect(screen.getByTestId("no-model-hint")).toBeInTheDocument();
    expect((screen.getByRole("button", { name: /^run$/i }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("no LLM running → the backend setup guide replaces the run surface", () => {
    useBackendStore.setState({ llamaHealthy: false});
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "llama_cpp", size_bytes: 1 }] });
    render(<Workspace />);
    expect(screen.getByTestId("backend-setup-guide")).toBeInTheDocument();
    expect(screen.getByTestId("setup-engine-llama_cpp")).toBeInTheDocument();
    expect(screen.queryByTestId("run-status")).toBeNull();
  });

  it("a healthy remote backend (vLLM) also shows the run surface, not the setup guide", () => {
    useBackendStore.setState({
      selectedBackend: "vllm",
      llamaHealthy: false,
      vllmHealthy: true,
    });
    useSelectedModelStore.setState({ selectedModels: [{ name: "Qwen/Qwen2.5-7B-Instruct-AWQ", backend: "vllm", size_bytes: 0 }] });
    render(<Workspace />);
    expect(screen.queryByTestId("backend-setup-guide")).toBeNull();
    expect(screen.getByTestId("run-status")).toBeInTheDocument();
  });

  it("a running LLM switches from the guide to the run surface", () => {
    useBackendStore.setState({ llamaHealthy: false});
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "llama_cpp", size_bytes: 1 }] });
    const { rerender } = render(<Workspace />);
    expect(screen.getByTestId("backend-setup-guide")).toBeInTheDocument();
    act(() => useBackendStore.setState({ llamaHealthy: true }));
    rerender(<Workspace />);
    expect(screen.queryByTestId("backend-setup-guide")).toBeNull();
    expect(screen.getByTestId("run-status")).toBeInTheDocument();
  });

  it("vLLM: Run is disabled until the remote server is reachable", () => {
    // llama.cpp is up (so the run surface renders, not the setup guide), but the
    // SELECTED backend is the unreachable remote — Run must still be blocked.
    useBackendStore.setState({ selectedBackend: "vllm", llamaHealthy: true, vllmHealthy: false });
    useSelectedModelStore.setState({ selectedModels: [{ name: "stub-remote", backend: "vllm", size_bytes: 0 }] });
    const { rerender } = render(<Workspace />);
    const runBtn = () => screen.getByRole("button", { name: /^run$/i }) as HTMLButtonElement;
    expect(runBtn().disabled).toBe(true);
    act(() => useBackendStore.setState({ vllmHealthy: true }));
    rerender(<Workspace />);
    expect(runBtn().disabled).toBe(false);
  });
});

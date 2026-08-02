import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";

import { ModelSelector } from "../ModelSelector";
import { useBackendStore } from "../shared/state/backendStore";
import { useSelectedModelStore } from "../shared/state/selectedModelStore";
import { useParamsStore } from "../shared/state/paramsStore";
import { useInstalledModelsStore } from "../features/models/state/installedModelsStore";
import type { InstalledModelInfo } from "../shared/ipc/models/storage";

const mk = (name: string, backend: InstalledModelInfo["backend"], path?: string): InstalledModelInfo => ({
  name, size_bytes: 100, modified_at: "", family: "llama", parameter_size: "1B",
  quantization: "Q4", backend, path,
});

const LIST: InstalledModelInfo[] = [
  mk("llama3.2:1b", "vllm"),
  mk("phi3:mini", "vllm"),
  mk("qwen.gguf", "llama_cpp", "/w/qwen.gguf"),
];

beforeEach(() => {
  useBackendStore.setState({ selectedBackend: "llama_cpp" });
  useSelectedModelStore.setState({ selectedModels: [] });
  useInstalledModelsStore.setState({ list: LIST, status: "ready", error: null });
});

describe("ModelSelector (global header)", () => {
  it("lists only models for the selected backend", () => {
    useBackendStore.setState({ selectedBackend: "vllm" });
    render(<ModelSelector />);
    fireEvent.click(screen.getByTestId("header-model-dropdown"));
    expect(screen.getByTestId("header-model-option-llama3.2:1b")).toBeInTheDocument();
    expect(screen.getByTestId("header-model-option-phi3:mini")).toBeInTheDocument();
    expect(screen.queryByTestId("header-model-option-qwen.gguf")).toBeNull();
  });

  it("single-select: a pick replaces, carrying backend + path", () => {
    useBackendStore.setState({ selectedBackend: "llama_cpp" });
    render(<ModelSelector />);
    fireEvent.click(screen.getByTestId("header-model-dropdown"));
    fireEvent.click(screen.getByTestId("header-model-option-qwen.gguf"));
    expect(useSelectedModelStore.getState().selectedModels).toEqual([
      { name: "qwen.gguf", backend: "llama_cpp", size_bytes: 100, path: "/w/qwen.gguf" },
    ]);
  });

  it("switching backend trims a now-mismatched selection (reconcile)", () => {
    useBackendStore.setState({ selectedBackend: "vllm" });
    render(<ModelSelector />);
    fireEvent.click(screen.getByTestId("header-model-dropdown"));
    fireEvent.click(screen.getByTestId("header-model-option-llama3.2:1b"));
    act(() => useBackendStore.getState().setSelectedBackend("llama_cpp"));
    expect(useSelectedModelStore.getState().selectedModels).toEqual([]);
    expect(screen.getByTestId("header-model-dropdown")).toHaveTextContent("Select a model");
  });

  it("shows an empty-state when the backend has no models", () => {
    useBackendStore.setState({ selectedBackend: "sglang" });
    render(<ModelSelector />);
    fireEvent.click(screen.getByTestId("header-model-dropdown"));
    expect(screen.getByText("No models for this backend.")).toBeInTheDocument();
  });

  it("holds the keep-model-loaded toggle inside the dropdown", () => {
    useParamsStore.setState({ keepLoaded: false });
    render(<ModelSelector />);
    expect(screen.queryByTestId("header-keep-loaded")).toBeNull(); // closed
    fireEvent.click(screen.getByTestId("header-model-dropdown"));
    const box = screen.getByTestId("header-keep-loaded") as HTMLInputElement;
    expect(box.checked).toBe(false);
    fireEvent.click(box);
    expect(useParamsStore.getState().keepLoaded).toBe(true);
  });
});

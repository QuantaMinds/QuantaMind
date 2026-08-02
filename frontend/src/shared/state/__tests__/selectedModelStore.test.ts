import { describe, it, expect, beforeEach } from "vitest";
import { useSelectedModelStore, type SelectedModel } from "../selectedModelStore";
import { useBackendStore } from "../backendStore";

const remoteA: SelectedModel = { name: "llama3.2:1b", backend: "vllm", size_bytes: 100 };
const remoteB: SelectedModel = { name: "mistral:7b", backend: "vllm", size_bytes: 300 };
const llamaModel: SelectedModel = { name: "qwen.gguf", backend: "llama_cpp", size_bytes: 200, path: "/w/qwen.gguf" };

beforeEach(() => {
  useSelectedModelStore.setState({ selectedModels: [] });
  useBackendStore.setState({ selectedBackend: "llama_cpp" });
});

describe("selectedModelStore", () => {
  it("starts empty and round-trips models with backend + path", () => {
    expect(useSelectedModelStore.getState().selectedModels).toEqual([]);
    useSelectedModelStore.getState().setSelectedModels([llamaModel]);
    expect(useSelectedModelStore.getState().selectedModels).toEqual([llamaModel]);
  });

  it("holds multiple models", () => {
    useSelectedModelStore.getState().setSelectedModels([remoteA, remoteB]);
    expect(useSelectedModelStore.getState().selectedModels).toHaveLength(2);
  });
});

describe("backend switch trims the global selection", () => {
  it("drops models whose backend no longer matches", () => {
    useSelectedModelStore.getState().setSelectedModels([remoteA, remoteB]);
    useBackendStore.getState().setSelectedBackend("llama_cpp");
    expect(useSelectedModelStore.getState().selectedModels).toEqual([]);
  });

  it("keeps models whose backend still matches", () => {
    useSelectedModelStore.getState().setSelectedModels([llamaModel]);
    useBackendStore.getState().setSelectedBackend("llama_cpp");
    expect(useSelectedModelStore.getState().selectedModels).toEqual([llamaModel]);
  });

  it("leaves an empty selection untouched", () => {
    useBackendStore.getState().setSelectedBackend("sglang");
    expect(useSelectedModelStore.getState().selectedModels).toEqual([]);
  });
});

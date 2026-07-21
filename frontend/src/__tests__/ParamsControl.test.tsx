import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";

vi.mock("../shared/memory/useVramFit", () => ({
  useVramFit: (model?: string) => ({
    dims: model ? { layers: 1, head_count: 1, head_count_kv: 1, embedding_length: 1, context_length: 131072 } : null,
    kvBytes: null,
  }),
}));

import { ParamsControl } from "../ParamsControl";
import { useParamsStore } from "../shared/state/paramsStore";
import { useBackendStore } from "../shared/state/backendStore";
import { useSelectedModelStore } from "../shared/state/selectedModelStore";

beforeEach(() => {
  useParamsStore.setState({ globalParams: {}, perModelParams: {}, sharedParams: true, keepLoaded: false });
  useBackendStore.setState({ selectedBackend: "ollama" });
  useSelectedModelStore.setState({ selectedModels: [] });
});

describe("ParamsControl (header global params)", () => {
  it("renders all six param rows when opened", () => {
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    for (const key of ["temperature", "top_p", "top_k", "max_tokens", "repeat_penalty", "seed"]) {
      expect(screen.getByTestId(`param-${key}`)).toBeInTheDocument();
    }
  });

  it("editing a row writes globalParams", () => {
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    fireEvent.change(screen.getByTestId("param-temperature-input"), { target: { value: "0.3" } });
    expect(useParamsStore.getState().globalParams.temperature).toBe(0.3);
  });

  it("shows a set-count badge and Reset all clears it", () => {
    useParamsStore.setState({ globalParams: { temperature: 0.3, seed: 7 } });
    render(<ParamsControl />);
    expect(screen.getByTestId("header-params-count")).toHaveTextContent("2");
    fireEvent.click(screen.getByTestId("header-params-button"));
    fireEvent.click(screen.getByTestId("header-params-reset"));
    expect(useParamsStore.getState().globalParams).toEqual({});
  });

  it("no per-model toggle for a single model", () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama", backend: "ollama", size_bytes: 1 }] });
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    expect(screen.queryByTestId("header-shared-params")).toBeNull();
  });

  it("2+ Ollama models: unchecking 'same for all' reveals per-model editors that write perModelParams", () => {
    useSelectedModelStore.setState({ selectedModels: [
      { name: "llama", backend: "ollama", size_bytes: 1 },
      { name: "mistral", backend: "ollama", size_bytes: 1 },
    ] });
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    // shared by default → no per-model groups
    expect(screen.getByTestId("header-shared-params")).toBeInTheDocument();
    expect(screen.queryByTestId("header-model-params-llama")).toBeNull();
    // uncheck → per-model groups appear
    fireEvent.click(screen.getByTestId("header-shared-params-toggle"));
    expect(screen.getByTestId("header-model-params-llama")).toBeInTheDocument();
    expect(screen.getByTestId("header-model-params-mistral")).toBeInTheDocument();
    // editing one model's row writes only that model's params
    const llamaTemp = within(screen.getByTestId("header-model-params-llama")).getByTestId("param-temperature-input");
    fireEvent.change(llamaTemp, { target: { value: "0.1" } });
    expect(useParamsStore.getState().perModelParams).toEqual({ llama: { temperature: 0.1 } });
  });

  it("offers 'Use max' for a single Ollama model and sets num_ctx to the model's context window", () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "phi3.5:latest", backend: "ollama", size_bytes: 1 }] });
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    expect(screen.getByTestId("header-ctx-max")).toHaveTextContent("131,072");
    fireEvent.click(screen.getByTestId("header-ctx-use-max"));
    expect(useParamsStore.getState().globalParams.num_ctx).toBe(131072);
  });

  it("dismisses on Escape", () => {
    render(<ParamsControl />);
    fireEvent.click(screen.getByTestId("header-params-button"));
    expect(screen.getByTestId("header-params-popover")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("header-params-popover")).toBeNull();
  });

  it("dismisses on outside click", () => {
    render(
      <div>
        <ParamsControl />
        <button type="button" data-testid="outside">outside</button>
      </div>,
    );
    fireEvent.click(screen.getByTestId("header-params-button"));
    expect(screen.getByTestId("header-params-popover")).toBeInTheDocument();
    fireEvent.mouseDown(screen.getByTestId("outside"));
    expect(screen.queryByTestId("header-params-popover")).toBeNull();
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act, within } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  default: ({
    value,
    onChange,
  }: {
    value?: string;
    onChange?: (v: string) => void;
  }) => (
    <textarea
      data-testid="prompt-input"
      value={value ?? ""}
      onChange={(e) => onChange?.(e.target.value)}
    />
  ),
}));

import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import App from "../App";
import { useWorkspacesStore } from "../features/workspaces/state/workspaceStore";
import { useBackendStore } from "../shared/state/backendStore";
import { useSelectedModelStore } from "../shared/state/selectedModelStore";
import { useCompareStore } from "../features/compare/state/compareStore";
import { useNavStore } from "../shared/state/navStore";
import { seedCurrentPrompt } from "./helpers/seedWorkspace";

const handlers: Record<string, EventCallback<unknown>> = {};

function fire(event: string, payload: unknown) {
  handlers[event]({ event, id: 0, payload });
}

beforeEach(() => {
  for (const k of Object.keys(handlers)) delete handlers[k];
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
  vi.mocked(listen).mockImplementation((event, cb) => {
    handlers[event] = cb as EventCallback<unknown>;
    return Promise.resolve(() => {
      delete handlers[event];
    });
  });
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "list_models")
      return Promise.resolve(["llama3.2:1b", "mistral:7b"]);
    if (cmd === "get_installed_models_with_stats")
      return Promise.resolve([
        { name: "llama3.2:1b", size_bytes: 1_000_000_000, modified_at: "", family: "llama", parameter_size: "1B", quantization: "Q4_K_M", backend: "ollama" },
        { name: "mistral:7b", size_bytes: 4_000_000_000, modified_at: "", family: "llama", parameter_size: "7B", quantization: "Q4_K_M", backend: "ollama" },
      ]);
    if (cmd === "check_ollama_health")
      return Promise.resolve({ available: true, version: "0.1.32" });
    if (cmd === "run_prompt") return Promise.resolve();
    if (cmd === "stop_prompt") return Promise.resolve();
    if (cmd === "save_prompt") return Promise.resolve(useWorkspacesStore.getState().current);
    return Promise.reject(new Error(`unknown ${cmd}`));
  });
  useBackendStore.setState({ selectedBackend: "ollama" });
  useSelectedModelStore.setState({ selectedModels: [] });
  useCompareStore.getState().reset();
  useNavStore.setState({ topView: "workspace", history: [] });
  seedCurrentPrompt();
});

describe("Top navigation layout", () => {
  it("has no Quant tab anywhere — the feature was removed (right-sizing lives on Agent Report)", async () => {
    render(<App />);
    await waitFor(() => expect(handlers["prompt-token"]).toBeDefined());
    expect(screen.queryByTestId("view-tab-quant")).toBeNull();
    fireEvent.click(screen.getByTestId("view-tab-compare"));
    expect(screen.queryByTestId("analysis-tab-quant")).toBeNull();
  });

  it("orders Models & Downloads after Agent Report", async () => {
    render(<App />);
    await waitFor(() => expect(handlers["prompt-token"]).toBeDefined());
    // Tests (eval) sits BEFORE Latency (inspector): the Latency page's Test-run view reads
    // the run you just launched, so the tab order mirrors the workflow (run → inspect).
    const order = ["workspace", "compare", "eval", "inspector", "audit", "agentReport", "models", "downloads", "settings", "docs"];
    const ids = order.map((id) => screen.getByTestId(`view-tab-${id}`));
    for (let i = 1; i < ids.length; i++) {
      expect(ids[i - 1].compareDocumentPosition(ids[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
  });
});

describe("Phase 1 E2E smoke — edit → run → re-run", () => {
  it("walks the full workspace flow with all four exit criteria", async () => {
    render(<App />);
    await waitFor(() =>
      expect(handlers["prompt-token"]).toBeDefined(),
    );

    // 1. EDIT — Workspace now has two PromptEditors (system + user); scope to user
    const userEditorWrap = await screen.findByTestId("user-prompt-editor");
    const editor = within(userEditorWrap).getByTestId("prompt-input");
    fireEvent.change(editor, { target: { value: "Why is the sky blue?" } });
    // Pick one model (a chip) → single-run mode
    fireEvent.click(await screen.findByTestId("header-model-dropdown"));
    fireEvent.click(await screen.findByTestId("header-model-option-llama3.2:1b"));

    // 2. RUN — navigates to Compare; the response streams into the M1 column
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("run_prompt", {
        model: "llama3.2:1b",
        prompt: "Why is the sky blue?",
        backend: "ollama",
      }),
    );
    expect(useNavStore.getState().topView).toBe("compare");
    act(() => {
      fire("prompt-token", { text: "The " });
      fire("prompt-token", { text: "sky " });
      fire("prompt-token", { text: "is " });
      fire("prompt-token", { text: "blue." });
    });
    act(() => {
      fire("prompt-done", { ttft_ms: 8, tokens_per_sec: 32.0, token_count: 4, timeline: [] });
    });
    const expected = "TTFT 8ms · 32.0 tok/s · 4 tokens";
    expect(await screen.findByTestId("compare-output-llama3.2:1b")).toHaveTextContent(
      "The sky is blue.",
    );
    expect(screen.getByTestId("compare-metrics-llama3.2:1b")).toHaveTextContent(expected);
    // The Workspace status bar still reflects the last run's metrics.
    expect(screen.getByTestId("status-bar-metrics")).toHaveTextContent(expected);
    expect(screen.getByTestId("run-status")).toHaveTextContent("done");

    // 3. RE-RUN from the Workspace succeeds without re-typing
    fireEvent.click(screen.getByTestId("view-tab-workspace"));
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    expect(invoke).toHaveBeenCalledWith("run_prompt", {
      model: "llama3.2:1b",
      prompt: "Why is the sky blue?",
      backend: "ollama",
    });
  });

  it("cancel mid-stream produces a distinct cancelled terminal state", async () => {
    render(<App />);
    await waitFor(() => expect(handlers["prompt-cancelled"]).toBeDefined());
    const userWrap = await screen.findByTestId("user-prompt-editor");
    fireEvent.change(within(userWrap).getByTestId("prompt-input"), {
      target: { value: "x" },
    });
    fireEvent.click(await screen.findByTestId("header-model-dropdown"));
    fireEvent.click(await screen.findByTestId("header-model-option-llama3.2:1b"));
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    expect(useNavStore.getState().topView).toBe("compare");
    act(() => fire("prompt-token", { text: "partial" }));
    // Cancel from the Workspace (where the run trigger lives).
    fireEvent.click(screen.getByTestId("view-tab-workspace"));
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_prompt"));
    act(() => fire("prompt-cancelled", { token_count: 1 }));
    expect(screen.getByTestId("run-status")).toHaveTextContent("cancelled");
    // A cancelled run shows no metrics in its Analysis column.
    expect(screen.queryByTestId("compare-metrics-llama3.2:1b")).toBeNull();
  });
});

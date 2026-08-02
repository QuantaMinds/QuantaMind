import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  default: () => <textarea data-testid="prompt-input" />,
}));

import { invoke } from "@tauri-apps/api/core";
import App from "../App";
import { useCompareStore } from "../features/compare/state/compareStore";
import { useBackendStore } from "../shared/state/backendStore";
import { useNavStore } from "../shared/state/navStore";
import { useModelStore } from "../features/models/state/modelStore";
import { useWorkspacesStore } from "../features/workspaces/state/workspaceStore";
import { seedCurrentPrompt } from "./helpers/seedWorkspace";

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "check_llama_health")
      return Promise.resolve({ available: true, version: null });
    if (cmd === "list_llama_models" || cmd === "list_vllm_models" || cmd === "list_sglang_models")
      return Promise.resolve([]);
    if (cmd === "get_disk_usage")
      return Promise.resolve({ total_bytes: 1, free_bytes: 1, models_bytes: 0 });
    if (cmd === "get_storage_path")
      return Promise.resolve({ current_path: "/tmp", from_env: false });
    if (cmd === "save_prompt") return Promise.resolve(useWorkspacesStore.getState().current);
    return Promise.reject(new Error(`unknown ${cmd}`));
  });
  useCompareStore.getState().reset();
  useBackendStore.setState({ selectedBackend: "llama_cpp", llamaHealthy: true });
  useNavStore.setState({ topView: "workspace" });
  useModelStore.setState({ downloads: {}, activeHfName: null, hfSearchQuery: "", hfSelectedRepo: null });
  seedCurrentPrompt();
});

const ALL = ["workspace", "inspector", "models", "downloads", "settings"] as const;

describe("App tab strip", () => {
  it("renders the tabs with Workspace active by default", () => {
    render(<App />);
    for (const id of ALL) {
      expect(screen.getByTestId(`view-tab-${id}`))
        .toHaveAttribute("aria-selected", id === "workspace" ? "true" : "false");
    }
    expect(screen.getByTestId("view-workspace")).not.toHaveAttribute("hidden");
    for (const id of ALL.filter((x) => x !== "workspace")) {
      expect(screen.getByTestId(`view-${id}`)).toHaveAttribute("hidden");
    }
  });

  it("exposes the two-zone nav: a Compare tab and an Audit tab", () => {
    render(<App />);
    // Zone 1 (Manual Playground) renamed Analysis → Compare; Zone 2 (Automated
    // Pipeline) gains a dedicated Audit tab alongside Eval.
    expect(screen.getByTestId("view-tab-compare")).toBeInTheDocument();
    expect(screen.getByTestId("view-tab-audit")).toBeInTheDocument();
  });

  it.each(ALL.filter((x) => x !== "workspace"))(
    "clicking the %s tab shows that view and hides Workspace",
    (id) => {
      render(<App />);
      fireEvent.click(screen.getByTestId(`view-tab-${id}`));
      expect(screen.getByTestId(`view-tab-${id}`)).toHaveAttribute("aria-selected", "true");
      expect(screen.getByTestId(`view-${id}`)).not.toHaveAttribute("hidden");
      expect(screen.getByTestId("view-workspace")).toHaveAttribute("hidden");
      expect(useNavStore.getState().topView).toBe(id);
    },
  );

  it("Workspace prompt survives the toggle (kept mounted via hidden)", () => {
    render(<App />);
    const userEditor = within(screen.getByTestId("user-prompt-editor"))
      .getByTestId("prompt-input") as HTMLTextAreaElement;
    fireEvent.change(userEditor, { target: { value: "ws value" } });
    fireEvent.click(screen.getByTestId("view-tab-models"));
    fireEvent.click(screen.getByTestId("view-tab-workspace"));
    const afterToggle = within(screen.getByTestId("user-prompt-editor"))
      .getByTestId("prompt-input") as HTMLTextAreaElement;
    expect(afterToggle.value).toBe("ws value");
  });
});

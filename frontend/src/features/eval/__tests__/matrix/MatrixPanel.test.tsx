import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../../shared/ipc/eval/matrix", () => ({ runCollectionMatrix: vi.fn(), loadCollectionHistory: vi.fn() }));
vi.mock("../../../../shared/ipc/eval/registry", () => ({ getBuiltinCollection: vi.fn(), loadCustomCollection: vi.fn() }));

import { MatrixPanel } from "../../components/matrix/MatrixPanel";
import { useEvalRegistryStore } from "../../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { getBuiltinCollection } from "../../../../shared/ipc/eval/registry";
import { runCollectionMatrix, loadCollectionHistory } from "../../../../shared/ipc/eval/matrix";

const tasks = [{
  id: "w", category: "single", prompt: "p",
  tools: [{ name: "x", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "call", name: "x", args: {} },
}];

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getBuiltinCollection).mockResolvedValue(tasks as never);
  vi.mocked(loadCollectionHistory).mockResolvedValue([]);
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: [],
    init: vi.fn().mockResolvedValue(undefined),
  });
  useInstalledModelsStore.setState({
    list: [{ name: "m1", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "ollama" }],
    status: "ready", error: null, lastRefreshedAt: 1,
  });
});

describe("MatrixPanel", () => {
  it("loads the active collection and runs a matrix for the toggled models", async () => {
    const report = {
      collection_id: "easy-coding", avg_score: 0.5,
      columns: [{ model: "m1", backend: "ollama", error: null,
        report: { n: 1, parse_rate: 1, tool_selection_acc: 1, arg_acc: 1, abstain_acc: null, composite: 0.5,
          per_task: [{ id: "w", category: "single", verdict: { parsed: true, tool_match: true, args_match: true, abstain_correct: null } }] } }],
    };
    vi.mocked(runCollectionMatrix).mockResolvedValue(report as never);

    render(<MatrixPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));

    expect(screen.getByTestId("matrix-run")).toBeDisabled(); // no model toggled yet
    fireEvent.click(screen.getByTestId("eval-model-dropdown")); // open the models menu
    fireEvent.click(screen.getByTestId("eval-model-toggle-m1"));
    await waitFor(() => expect(screen.getByTestId("matrix-run")).not.toBeDisabled());

    fireEvent.click(screen.getByTestId("matrix-run"));
    await waitFor(() => expect(runCollectionMatrix).toHaveBeenCalledOnce());
    const [collectionId, targets, passedTasks] = vi.mocked(runCollectionMatrix).mock.calls[0];
    expect(collectionId).toBe("easy-coding");
    expect(targets).toEqual([{ model: "m1", backend: "ollama" }]);
    expect(passedTasks).toHaveLength(1);
    await waitFor(() => expect(screen.getByTestId("eval-matrix-cell-w-m1")).toBeTruthy());
  });

  it("switches to the timeline view", async () => {
    render(<MatrixPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    fireEvent.click(screen.getByTestId("matrix-view-timeline"));
    expect(screen.getByTestId("eval-history-empty")).toBeTruthy();
  });
});

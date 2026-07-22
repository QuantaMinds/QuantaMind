import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../shared/ipc/eval/toolcall", () => ({ runToolcallEval: vi.fn() }));
vi.mock("../../../shared/ipc/eval/registry", () => ({ getBuiltinCollection: vi.fn(), loadCustomCollection: vi.fn() }));

import { runToolcallEval } from "../../../shared/ipc/eval/toolcall";
import { getBuiltinCollection } from "../../../shared/ipc/eval/registry";
import { ToolCallPanel } from "../components/ToolCallPanel";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useEvalRegistryStore } from "../state/evalRegistryStore";

const builtinTasks = [
  { id: "weather", category: "single", prompt: "p", tools: [{ name: "get_weather", description: "", parameters: { type: "object", properties: {} } }], expected: { type: "call", name: "get_weather", args: {} } },
] as never;

const report = {
  n: 2,
  parse_rate: 1, tool_selection_acc: 0.5, arg_acc: 0.5, abstain_acc: 1, composite: 0.75, prompt_tokens: null,
  per_task: [
    { id: "weather", category: "single", verdict: { parsed: true, tool_match: true, args_match: false, abstain_correct: null } },
    { id: "abstain-1", category: "abstain", verdict: { parsed: false, tool_match: false, args_match: false, abstain_correct: true } },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getBuiltinCollection).mockResolvedValue(builtinTasks as never);
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: [],
    init: vi.fn().mockResolvedValue(undefined),
  });
  useInstalledModelsStore.setState({
    list: [{ name: "m", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "ollama" }],
    status: "ready", error: null, lastRefreshedAt: 1,
  });
});

describe("ToolCallPanel", () => {
  it("renders the four sub-scores + composite and a per-task table", async () => {
    vi.mocked(runToolcallEval).mockResolvedValue(report);
    render(<ToolCallPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));
    fireEvent.change(screen.getByTestId("toolcall-model-select"), { target: { value: "m" } });
    await waitFor(() => expect(screen.getByTestId("toolcall-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("toolcall-run"));
    await waitFor(() => expect(screen.getByTestId("toolcall-scores")).toHaveTextContent("Composite 75%"));
    expect(screen.getByTestId("toolcall-scores")).toHaveTextContent("Tool 50%");
    expect(screen.getByTestId("toolcall-row-weather")).toBeTruthy();
    expect(screen.getByTestId("toolcall-row-abstain-1")).toHaveTextContent("abstained");
  });

  it("shows 'Not available' on a backend error (no fabricated score)", async () => {
    vi.mocked(runToolcallEval).mockRejectedValue(new Error("backend down"));
    render(<ToolCallPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("toolcall-model-select"), { target: { value: "m" } });
    await waitFor(() => expect(screen.getByTestId("toolcall-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("toolcall-run"));
    await waitFor(() => expect(screen.getByTestId("toolcall-error")).toHaveTextContent("Not available"));
    expect(screen.queryByTestId("toolcall-scores")).toBeNull();
  });

  it("View Trace hands the task off (collection + taskId + model)", async () => {
    const onViewTrace = vi.fn();
    render(<ToolCallPanel onViewTrace={onViewTrace} />);
    await waitFor(() => expect(screen.getByTestId("toolcall-row-weather")).toBeTruthy());
    fireEvent.change(screen.getByTestId("toolcall-model-select"), { target: { value: "m" } });
    fireEvent.click(screen.getByTestId("toolcall-view-trace-weather"));
    expect(onViewTrace).toHaveBeenCalledWith({ collection: "easy-coding", taskId: "weather", model: "m" });
  });
});

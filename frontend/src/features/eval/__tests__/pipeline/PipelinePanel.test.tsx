import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../../shared/ipc/eval/registry", () => ({ getBuiltinCollection: vi.fn(), loadCustomCollection: vi.fn() }));
vi.mock("../../../../shared/ipc/eval/toolcall", async (orig) => ({
  ...(await orig<typeof import("../../../../shared/ipc/eval/toolcall")>()),
  traceToolcallTask: vi.fn(),
  loadToolcallTrace: vi.fn(),
}));

import { PipelinePanel } from "../../components/pipeline/PipelinePanel";
import { useEvalRegistryStore } from "../../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { getBuiltinCollection } from "../../../../shared/ipc/eval/registry";
import { traceToolcallTask, loadToolcallTrace } from "../../../../shared/ipc/eval/toolcall";

const tasks = [{
  id: "w", category: "single", prompt: "Weather in London?",
  tools: [{ name: "get_weather", description: "", parameters: { type: "object", properties: { city: { type: "string" } } } }],
  expected: { type: "call", name: "get_weather", args: { city: "London" } },
}];

const trace = {
  system_message: "You can call tools. Available tools: [...]",
  user_prompt: "Weather in London?",
  raw_output: '{"name": "get_weather", "args": {"city": "London"}}',
  verdict: { parsed: true, tool_match: true, args_match: true, abstain_correct: null },
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getBuiltinCollection).mockResolvedValue(tasks as never);
  vi.mocked(traceToolcallTask).mockResolvedValue(trace as never);
  vi.mocked(loadToolcallTrace).mockResolvedValue(null);
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

describe("PipelinePanel", () => {
  it("runs a single task trace and reveals real prompt + output + verdict", async () => {
    render(<PipelinePanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));

    // Config phase shows the task prompt before running.
    expect(screen.getByTestId("pipeline-config")).toHaveTextContent("Weather in London?");
    expect(screen.getByTestId("pipeline-run")).toBeDisabled(); // no model yet

    fireEvent.change(screen.getByTestId("pipeline-model-select"), { target: { value: "m1" } });
    await waitFor(() => expect(screen.getByTestId("pipeline-run")).not.toBeDisabled());

    fireEvent.click(screen.getByTestId("pipeline-run"));
    await waitFor(() => expect(traceToolcallTask).toHaveBeenCalledOnce());
    const [model, backend, passedTask] = vi.mocked(traceToolcallTask).mock.calls[0];
    expect(model).toBe("m1");
    expect(backend).toBe("ollama");
    expect(passedTask.id).toBe("w");

    await waitFor(() => expect(screen.getByTestId("pipeline-validation")).toHaveTextContent("PASSED"));

    // Step to System Pkg → real system message; Stream → real raw output; Verify → report.
    fireEvent.click(screen.getByTestId("pipeline-next"));
    expect(screen.getByTestId("pipeline-system")).toHaveTextContent("You can call tools");
    fireEvent.click(screen.getByTestId("pipeline-next"));
    expect(screen.getByTestId("pipeline-stream")).toHaveTextContent('"name": "get_weather"');
    fireEvent.click(screen.getByTestId("pipeline-next"));
    expect(screen.getByTestId("pipeline-verify-success")).toHaveTextContent("100% SUCCESS");
  });

  it("resets the trace", async () => {
    render(<PipelinePanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("pipeline-model-select"), { target: { value: "m1" } });
    fireEvent.click(screen.getByTestId("pipeline-run"));
    await waitFor(() => expect(screen.getByTestId("pipeline-validation")).toHaveTextContent("PASSED"));
    fireEvent.click(screen.getByTestId("pipeline-reset"));
    expect(screen.getByTestId("pipeline-validation")).toHaveTextContent("Pending");
    expect(screen.getByTestId("pipeline-exec-state")).toHaveTextContent("Idle");
  });

  it("applies a Scoreboard handoff (focus loads the collection/task/model)", async () => {
    render(<PipelinePanel focus={{ collection: "easy-coding", taskId: "w", model: "m1" }} />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));
    await waitFor(() => expect(screen.getByTestId("pipeline-model-select")).toHaveValue("m1"));
    expect(screen.getByTestId("pipeline-task-select")).toHaveValue("w");
  });

  it("shows a cached trace on handoff WITHOUT re-running inference", async () => {
    vi.mocked(loadToolcallTrace).mockResolvedValue(trace as never);
    render(<PipelinePanel focus={{ collection: "easy-coding", taskId: "w", model: "m1" }} />);

    await waitFor(() => expect(loadToolcallTrace).toHaveBeenCalledWith("easy-coding", "m1", "w"));
    // The saved verdict + output are shown straight away…
    await waitFor(() => expect(screen.getByTestId("pipeline-exec-state")).toHaveTextContent("Cached"));
    expect(screen.getByTestId("pipeline-validation")).toHaveTextContent("PASSED");
    expect(screen.getByTestId("pipeline-from-cache")).toBeInTheDocument();
    // …and no live trace was run.
    expect(traceToolcallTask).not.toHaveBeenCalled();

    // The real raw output is reachable by stepping to the Stream phase.
    fireEvent.click(screen.getByTestId("pipeline-next"));
    fireEvent.click(screen.getByTestId("pipeline-next"));
    expect(screen.getByTestId("pipeline-stream")).toHaveTextContent('"name": "get_weather"');
  });
});

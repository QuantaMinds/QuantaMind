import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// The scoreboard + debugger have their own suites; stub them so this stays a
// page-composition check (3 panes mount + registry initialises).
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
// Expose the model/task the page hands each pane, so the nav-persistence tests can assert the
// results view stays bound to the RUN (not detached to a blank model/task) across selection churn.
vi.mock("../components/scoreboard/MatrixScoreboard", () => ({
  MatrixScoreboard: (p: { model: string; focusedTaskId: string | null; runTaskId?: string | null }) => (
    <div data-testid="matrix-scoreboard" data-model={p.model} data-task={p.focusedTaskId ?? ""} data-runtask={p.runTaskId ?? ""} />
  ),
}));
vi.mock("../components/TraceDebugger", () => ({
  TraceDebugger: (p: { model: string; taskId: string | null }) => (
    <div data-testid="trace-debugger" data-model={p.model} data-task={p.taskId ?? ""} />
  ),
}));
// Auto resolves to Medium so the k-prefill tests can exercise the Auto one-shot.
vi.mock("../../../shared/ipc/compare/hardware", () => ({
  getHardwareTier: vi.fn().mockResolvedValue({ total_memory_bytes: 16 * 1024 ** 3, class: "Mainstream", recommended_tier: "medium" }),
}));

import { EvalPage } from "../components/EvalPage";
import { useEvalRegistryStore } from "../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBatchStore } from "../state/batchStore";
import { useCliffStore } from "../state/cliffStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";

const init = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
  useBatchStore.getState().reset();
  useBackendStore.setState({ selectedBackend: "llama_cpp" });
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: [],
    selected: "easy-coding",
    tasks: [],
    init,
  });
  useInstalledModelsStore.setState({
    list: [{ name: "llama3.2:1b", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "llama_cpp" }],
    status: "ready",
    error: null,
    lastRefreshedAt: 1,
  });
});

describe("EvalPage (3-pane workspace)", () => {
  it("mounts the Manager, Matrix Scoreboard and Trace Debugger, and inits the registry", async () => {
    render(<EvalPage />);
    expect(await screen.findByTestId("eval-manager")).toBeTruthy();
    expect(screen.getByTestId("matrix-scoreboard")).toBeTruthy();
    expect(screen.getByTestId("trace-debugger")).toBeTruthy();
    expect(init).toHaveBeenCalled();
  });

  it("disables the audit export until a batch report exists", () => {
    render(<EvalPage />);
    expect(screen.getByTestId("export-csv")).toBeDisabled();
  });

  it("clears the last run's results when the backend changes (no stale models)", () => {
    render(<EvalPage />);
    act(() =>
      useBatchStore.setState({
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "llama_cpp", toolcall: null, agentic: null, error: null }] },
      }),
    );
    expect(useBatchStore.getState().report).not.toBeNull();
    act(() => useBackendStore.setState({ selectedBackend: "vllm" }));
    expect(useBatchStore.getState().report).toBeNull();
  });

  it("clears the last run's results when the COLLECTION changes (no stale Pass/Fail leak)", () => {
    render(<EvalPage />);
    act(() =>
      useBatchStore.setState({
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "llama_cpp", toolcall: null, agentic: null, error: null }] },
        outcomeByKey: { "llama3.2:1b weather": { kind: "single", passed: true, trace: {} } } as never,
      }),
    );
    expect(useBatchStore.getState().report).not.toBeNull();
    // Switching to another collection must wipe the previous collection's outcomes.
    act(() => useEvalRegistryStore.setState({ selected: "finance" }));
    expect(useBatchStore.getState().report).toBeNull();
    expect(useBatchStore.getState().outcomeByKey).toEqual({});
  });

  // Nav-persistence: a Tests-page selection change must NEVER cancel a run on another
  // surface. The Audit cliff owns its own model/collection + Stop.
  it("does NOT halt a running Context-Cliff probe when the collection changes (nav-persistence)", () => {
    render(<EvalPage />);
    // A probe running (started from the Audit tab) for a different collection.
    act(() => useCliffStore.setState({ running: true, runningModel: "llama3.2:1b" }));
    act(() => useEvalRegistryStore.setState({ selected: "finance" }));
    expect(useCliffStore.getState().running).toBe(true);
    expect(useCliffStore.getState().runningModel).toBe("llama3.2:1b");
  });

  it("does NOT halt a running Context-Cliff probe when the (global) backend changes", () => {
    render(<EvalPage />);
    act(() => useCliffStore.setState({ running: true, runningModel: "llama3.2:1b" }));
    // The backend is global — this fires even from the Workspace model picker.
    act(() => useBackendStore.setState({ selectedBackend: "llama_cpp" }));
    expect(useCliffStore.getState().running).toBe(true);
  });

  it("does NOT clear a RUNNING batch's live report on a backend switch (the run keeps streaming)", () => {
    render(<EvalPage />);
    act(() =>
      useBatchStore.setState({
        running: true,
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "llama_cpp", toolcall: null, agentic: null, error: null }] },
      }),
    );
    act(() => useBackendStore.setState({ selectedBackend: "llama_cpp" }));
    expect(useBatchStore.getState().report).not.toBeNull();
    expect(useBatchStore.getState().running).toBe(true);
  });
});

describe("EvalPage — k pre-fill from tier (no clobber)", () => {
  beforeEach(() => {
    useEvalRegistryStore.setState({
      presets: [
        { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" },
        { id: "medium-coding", label: "Coding", domain: "coding", tier: "medium" },
        { id: "hard-coding", label: "Coding", domain: "coding", tier: "hard" },
      ],
      collections: [],
      selected: "medium-coding",
      tasks: [],
      init,
      select: vi.fn().mockResolvedValue(undefined),
      isPreset: (v: string) => ["easy-coding", "medium-coding", "hard-coding"].includes(v),
    });
    useInstalledModelsStore.setState({
      list: [{ name: "llama3.2:1b", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "llama_cpp" }],
      status: "ready", error: null, lastRefreshedAt: 1,
    });
  });

  it("pre-fills k to the tier's recommended value when a concrete tier is picked", async () => {
    render(<EvalPage />);
    await screen.findByTestId("eval-manager-k");
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(screen.getByTestId("eval-manager-k")).toHaveValue(16); // pass_k_for(Hard)
  });

  it("does NOT clobber a manually-typed k on an unrelated re-render after Auto resolves", async () => {
    render(<EvalPage />);
    // Auto resolves (hwTier → medium) → the one-shot fills k = 8.
    await waitFor(() => expect(screen.getByTestId("eval-manager-k")).toHaveValue(8));
    // User overrides k.
    fireEvent.change(screen.getByTestId("eval-manager-k"), { target: { value: "12" } });
    expect(screen.getByTestId("eval-manager-k")).toHaveValue(12);
    // An unrelated re-render (change Max Steps) must NOT reset k — the regression guard.
    fireEvent.change(screen.getByTestId("eval-manager-max-steps"), { target: { value: "10" } });
    expect(screen.getByTestId("eval-manager-k")).toHaveValue(12);
  });

  it("re-pre-fills k when the user toggles back to Auto (one-shot re-arms)", async () => {
    render(<EvalPage />);
    await waitFor(() => expect(screen.getByTestId("eval-manager-k")).toHaveValue(8)); // auto → medium
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(screen.getByTestId("eval-manager-k")).toHaveValue(16);
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "auto" } });
    await waitFor(() => expect(screen.getByTestId("eval-manager-k")).toHaveValue(8)); // re-armed → recommended
  });

  it("pre-fills Max Steps to the tier's recommended budget when a concrete tier is picked", async () => {
    render(<EvalPage />);
    await screen.findByTestId("eval-manager-max-steps");
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(32); // max_steps_for(Hard)
  });

  it("does NOT clobber a manually-typed Max Steps when k changes (independent suppress)", async () => {
    render(<EvalPage />);
    // Auto resolves (hwTier → medium) → the one-shot fills Max Steps = 16.
    await waitFor(() => expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(16));
    // User overrides Max Steps.
    fireEvent.change(screen.getByTestId("eval-manager-max-steps"), { target: { value: "20" } });
    expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(20);
    // Editing k must NOT reset Max Steps — the suppress refs are independent.
    fireEvent.change(screen.getByTestId("eval-manager-k"), { target: { value: "12" } });
    expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(20);
  });

  it("re-pre-fills Max Steps when the user toggles back to Auto (one-shot re-arms)", async () => {
    render(<EvalPage />);
    await waitFor(() => expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(16)); // auto → medium
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(32);
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "auto" } });
    await waitFor(() => expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(16)); // re-armed
  });

  it("per-task Delete from the sidebar opens the confirm dialog (built-in → saves a copy)", async () => {
    useEvalRegistryStore.setState({
      tasks: [{ id: "t1", category: "single", prompt: "p", tools: [{ name: "x", description: "", parameters: { type: "object", properties: {} } }], expected: { type: "call", name: "x", args: {} } }] as never,
    });
    render(<EvalPage />);
    // Click the collection to expand its task list, then delete a task.
    fireEvent.click(await screen.findByTestId("eval-collection-item-medium-coding"));
    const row = await screen.findByTestId("eval-task-row-t1");
    fireEvent.mouseEnter(row);
    fireEvent.click(screen.getByTestId("eval-task-delete-t1"));
    const dialog = screen.getByTestId("confirm-dialog");
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveTextContent(/copy/i); // built-in collection → editable copy
  });
});

// The bug: while a batch is running, switching screens (which changes the GLOBAL model/backend)
// or switching the difficulty tier detached the live Scoreboard/Trace from the run — the panes
// re-pointed to a blank/other model+task, so "which task is running" and the live trace went blank.
// The results view must stay bound to the run until it ends (the page's own nav-persistence law).
describe("EvalPage — results view stays bound to a running batch (nav-persistence)", () => {
  const select = vi.fn().mockResolvedValue(undefined);
  beforeEach(() => {
    select.mockClear();
    useSelectedModelStore.setState({
      selectedModels: [{ name: "llama3.2:1b", backend: "llama_cpp", size_bytes: 1 }],
    });
    useEvalRegistryStore.setState({
      presets: [
        { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" },
        { id: "hard-coding", label: "Coding", domain: "coding", tier: "hard" },
      ],
      collections: [],
      selected: "easy-coding",
      tasks: [],
      init,
      select,
      isPreset: (v: string) => ["easy-coding", "hard-coding"].includes(v),
    });
  });

  it("keeps the focused task on the LIVE task when the collection changes mid-run", async () => {
    render(<EvalPage />);
    // A running batch whose live activity is task t1 → the page auto-follows it into the Trace.
    act(() => useBatchStore.setState({ running: true, live: { ...useBatchStore.getState().live, taskId: "t1" } }));
    await waitFor(() => expect(screen.getByTestId("trace-debugger").getAttribute("data-task")).toBe("t1"));
    // A collection change mid-run (what a tier switch triggers) must NOT null the focus.
    act(() => useEvalRegistryStore.setState({ selected: "hard-coding" }));
    expect(screen.getByTestId("trace-debugger").getAttribute("data-task")).toBe("t1");
  });

  it("keeps the focused model on the RUN's model when the global backend changes mid-run", async () => {
    render(<EvalPage />);
    await waitFor(() => expect(screen.getByTestId("matrix-scoreboard").getAttribute("data-model")).toBe("llama3.2:1b"));
    act(() => useBatchStore.setState({ running: true }));
    // The backend is global — this fires even from the Workspace picker while the user is away.
    act(() => useBackendStore.setState({ selectedBackend: "llama_cpp" }));
    expect(screen.getByTestId("matrix-scoreboard").getAttribute("data-model")).toBe("llama3.2:1b");
  });

  it("does NOT swap the selected collection when the tier changes mid-run", async () => {
    render(<EvalPage />);
    await screen.findByTestId("eval-manager");
    act(() => useBatchStore.setState({ running: true }));
    select.mockClear();
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(select).not.toHaveBeenCalled();
    expect(useEvalRegistryStore.getState().selected).toBe("easy-coding");
  });

  it("(regression) STILL clears focus + swaps collection when IDLE (the run has ended)", async () => {
    render(<EvalPage />);
    await waitFor(() => expect(screen.getByTestId("trace-debugger").getAttribute("data-model")).toBe("llama3.2:1b"));
    // Idle (running=false): a tier switch is free to re-target the collection to the new tier.
    fireEvent.change(screen.getByTestId("eval-tier-dropdown"), { target: { value: "hard" } });
    expect(select).toHaveBeenCalledWith("hard-coding");
  });
});

// Single-task run scope: selecting one task in the sidebar threads a `runTaskId` to the Simulator
// (which scopes its rows) and previews it in the Evaluator; a collection change drops the scope.
describe("EvalPage — single-task run scope", () => {
  const select = vi.fn().mockResolvedValue(undefined);
  beforeEach(() => {
    select.mockClear();
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "llama_cpp", size_bytes: 1 }] });
    // Tier "medium" so it survives the Built-in list's tier filter (hwTier mock → medium).
    useEvalRegistryStore.setState({
      presets: [{ id: "med-coding", label: "Coding", domain: "coding", tier: "medium" }],
      collections: [], selected: "med-coding",
      tasks: [{ id: "t1", category: "single", prompt: "p", tools: [{ name: "x", description: "", parameters: { type: "object", properties: {} } }], expected: { type: "call", name: "x", args: {} } }] as never,
      init, select, isPreset: (v: string) => v === "med-coding",
    });
  });

  it("selecting a task threads runTaskId to the Simulator and previews it in the Evaluator", async () => {
    render(<EvalPage />);
    // Expand the collection's task list, then click the task to select it.
    fireEvent.click(await screen.findByTestId("eval-collection-item-med-coding"));
    fireEvent.click(await screen.findByTestId("eval-task-row-t1"));
    expect(screen.getByTestId("matrix-scoreboard").getAttribute("data-runtask")).toBe("t1");
    expect(screen.getByTestId("trace-debugger").getAttribute("data-task")).toBe("t1");
  });

  it("drops the single-task scope when the collection changes (no stale id leaks across collections)", async () => {
    render(<EvalPage />);
    fireEvent.click(await screen.findByTestId("eval-collection-item-med-coding"));
    fireEvent.click(await screen.findByTestId("eval-task-row-t1"));
    expect(screen.getByTestId("matrix-scoreboard").getAttribute("data-runtask")).toBe("t1");
    act(() => useEvalRegistryStore.setState({ selected: "other-coding" }));
    expect(screen.getByTestId("matrix-scoreboard").getAttribute("data-runtask")).toBe("");
  });
});

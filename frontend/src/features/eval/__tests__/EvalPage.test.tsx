import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// The scoreboard + debugger have their own suites; stub them so this stays a
// page-composition check (3 panes mount + registry initialises).
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../components/scoreboard/MatrixScoreboard", () => ({ MatrixScoreboard: () => <div data-testid="matrix-scoreboard" /> }));
vi.mock("../components/TraceDebugger", () => ({ TraceDebugger: () => <div data-testid="trace-debugger" /> }));
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

const init = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
  useBatchStore.getState().reset();
  useBackendStore.setState({ selectedBackend: "ollama" });
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: [],
    selected: "easy-coding",
    tasks: [],
    init,
  });
  useInstalledModelsStore.setState({
    list: [{ name: "llama3.2:1b", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "ollama" }],
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
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "ollama", toolcall: null, agentic: null, error: null }] },
      }),
    );
    expect(useBatchStore.getState().report).not.toBeNull();
    act(() => useBackendStore.setState({ selectedBackend: "llama_cpp" }));
    expect(useBatchStore.getState().report).toBeNull();
  });

  it("clears the last run's results when the COLLECTION changes (no stale Pass/Fail leak)", () => {
    render(<EvalPage />);
    act(() =>
      useBatchStore.setState({
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "ollama", toolcall: null, agentic: null, error: null }] },
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
        report: { collection_id: "easy-coding", columns: [{ model: "llama3.2:1b", backend: "ollama", toolcall: null, agentic: null, error: null }] },
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
      list: [{ name: "llama3.2:1b", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "ollama" }],
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

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../../../../shared/ipc/eval/batch", () => ({
  runBatchEval: vi.fn(),
  stopBatchEval: vi.fn(),
  EVENT_BATCH_PROGRESS: "batch-progress",
  EVENT_AGENTIC_STEP: "agentic-step",
  EVENT_BATCH_COMPLETE: "batch-complete",
  THINK_PRESET_TOKENS: {
    lean: { easy: 1024, medium: 2048, hard: 4096, extreme: 6144 },
    standard: { easy: 2048, medium: 6144, hard: 10240, extreme: 16384 },
    deep: { easy: 4096, medium: 12288, hard: 20480, extreme: 32768 },
  },
}));
// The batch run pre-flights the backend's health; treat it as up here so the run
// proceeds (the pre-flight itself is tested in useBatchRun.test).
vi.mock("../../../../shared/ipc/core/client", () => ({
  healthFor: vi.fn().mockResolvedValue({ available: true, version: null }),
}));

import { EvalManager } from "../../components/manager/EvalManager";
import { useEvalRegistryStore } from "../../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { useSelectedModelStore } from "../../../../shared/state/selectedModelStore";
import { runBatchEval } from "../../../../shared/ipc/eval/batch";
import { useParamsStore } from "../../../../shared/state/paramsStore";
import { useBackendStore } from "../../../../shared/state/backendStore";
import { useBatchStore } from "../../state/batchStore";

const sampleTasks = [{
  id: "w",
  category: "single",
  prompt: "Weather in Paris?",
  tools: [{ name: "get_weather", description: "Get weather", parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] } }],
  expected: { type: "call", name: "get_weather", args: { city: "Paris" } },
}] as never;

const init = vi.fn().mockResolvedValue(undefined);
const select = vi.fn().mockResolvedValue(undefined);
const importFile = vi.fn().mockResolvedValue(undefined);

// Default props shorthand: the eval model + collection config (the model comes from
// the GLOBAL selection store; the prop is the chosen one).
const props = (over: Record<string, unknown> = {}) => ({
  model: "llama3.2:1b",
  setModel: () => {},
  k: 1,
  setK: () => {},
  maxSteps: 8,
  setMaxSteps: () => {},
  // The Built-In list shows only the chosen tier's collections; the default preset
  // (easy-coding) is Easy, so default the resolved tier to "easy" for the picker.
  tierSel: "easy" as const,
  effectiveTier: "easy" as const,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  // A mocked run never receives its batch-complete event, so `running` would stay
  // true and the next click would hit the Stop path — reset it between tests.
  useBatchStore.getState().reset();
  useBackendStore.setState({ selectedBackend: "ollama" });
  useInstalledModelsStore.setState({
    list: [{ name: "llama3.2:1b", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "Q4_0", backend: "ollama" }],
    status: "ready", error: null, lastRefreshedAt: 1,
  });
  // The eval model dropdown is driven by the GLOBAL selection (not a per-page list).
  useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "ollama", size_bytes: 1 }] });
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: ["my-evals"],
    selected: "easy-coding",
    tasks: sampleTasks,
    init,
    select,
    importFile,
  });
});

describe("EvalManager Sidebar Controls", () => {
  it("lists the GLOBAL selection in the Model dropdown (single source of truth)", () => {
    useSelectedModelStore.setState({
      selectedModels: [
        { name: "qwen3.5:9b", backend: "ollama", size_bytes: 1 },
        { name: "llama3.2:1b", backend: "ollama", size_bytes: 1 },
      ],
    });
    render(<EvalManager {...props({ model: "qwen3.5:9b" })} />);
    const dropdown = screen.getByTestId("eval-model-dropdown");
    expect(dropdown).toHaveTextContent("qwen3.5:9b");
    expect(dropdown).toHaveTextContent("llama3.2:1b");
  });

  it("prompts to pick a model at the top when nothing is selected globally", () => {
    useSelectedModelStore.setState({ selectedModels: [] });
    render(<EvalManager {...props({ model: "" })} />);
    expect(screen.getByTestId("eval-no-model")).toHaveTextContent("Select a model at the top");
    expect(screen.queryByTestId("eval-model-dropdown")).toBeNull();
    expect(screen.getByTestId("eval-run-all")).toHaveAttribute("title", "Select a model at the top");
  });

  it("offers an ⓘ next to Iterations (k) explaining Pass^k", () => {
    render(<EvalManager {...props()} />);
    const info = screen.getByTestId("info-iterations");
    fireEvent.mouseEnter(info.parentElement as HTMLElement);
    expect(screen.getByTestId("info-popup-iterations")).toHaveTextContent(/Pass\^k/);
  });

  it("shows the llama.cpp jinja note under native tool-calling, hidden for Ollama", () => {
    // Ollama (set in beforeEach): the Ollama view is unchanged — no llama note.
    const { rerender } = render(<EvalManager {...props()} />);
    expect(screen.queryByTestId("eval-method-llama-jinja-note")).toBeNull();
    // Switch the running backend to llama.cpp → the jinja/template note appears.
    useBackendStore.setState({ selectedBackend: "llama_cpp" });
    rerender(<EvalManager {...props()} />);
    expect(screen.getByTestId("eval-method-llama-jinja-note")).toHaveTextContent(/jinja/i);
  });

  it("explains WHY the RUN BATCH button is disabled (no model vs no tasks)", () => {
    useSelectedModelStore.setState({ selectedModels: [] });
    const { rerender } = render(<EvalManager {...props({ model: "" })} />);
    expect(screen.getByTestId("eval-run-all")).toHaveAttribute("title", "Select a model at the top");
    // Model present but the collection has no tasks → the button says no tasks.
    useSelectedModelStore.setState({ selectedModels: [{ name: "llama3.2:1b", backend: "ollama", size_bytes: 1 }] });
    useEvalRegistryStore.setState({ tasks: [] });
    rerender(<EvalManager {...props()} />);
    expect(screen.getByTestId("eval-run-all")).toHaveAttribute("title", "This collection has no tasks");
  });

  it("calling method: Tool-Calling is default-on, Prompt-based off; unticking both blocks the run", () => {
    render(<EvalManager {...props()} />);
    const native = screen.getByTestId("eval-method-native") as HTMLInputElement;
    const prompt = screen.getByTestId("eval-method-prompt") as HTMLInputElement;
    // Tool-Calling default-on, Prompt-based default-off.
    expect(native.checked).toBe(true);
    expect(prompt.checked).toBe(false);
    // Untick the only selected method → "pick at least one" hint + RUN disabled.
    fireEvent.click(native);
    expect(screen.getByTestId("eval-method-none-hint")).toBeInTheDocument();
    expect(screen.getByTestId("eval-run-all")).toBeDisabled();
    // Pick Prompt-based → hint gone, RUN enabled again.
    fireEvent.click(prompt);
    expect(screen.queryByTestId("eval-method-none-hint")).toBeNull();
    expect(screen.getByTestId("eval-run-all")).not.toBeDisabled();
  });

  it("renders the headers and Data Source radio controls", () => {
    render(<EvalManager {...props()} />);
    expect(screen.getByText("1. TESTS MANAGER")).toBeInTheDocument();
    expect(screen.getByText("(File & Controls)")).toBeInTheDocument();
    expect(screen.getByText("◉ Built-in")).toBeInTheDocument();
    expect(screen.getByText("◯ Custom JSON")).toBeInTheDocument();
  });

  it("renders presets under collections when Built-in is selected", () => {
    render(<EvalManager {...props()} />);
    expect(screen.getByTestId("eval-collection-item-easy-coding")).toBeInTheDocument();
  });

  it("shows ONLY the chosen tier's built-in collections", () => {
    useEvalRegistryStore.setState({
      presets: [
        { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" },
        { id: "medium-coding", label: "Coding", domain: "coding", tier: "medium" },
        { id: "hard-coding", label: "Coding", domain: "coding", tier: "hard" },
      ],
      collections: [],
      selected: "medium-coding",
      tasks: sampleTasks,
      init,
      select,
      importFile,
    });
    render(<EvalManager {...props({ tierSel: "medium", effectiveTier: "medium" })} />);
    expect(screen.getByTestId("eval-collection-item-medium-coding")).toBeInTheDocument();
    expect(screen.queryByTestId("eval-collection-item-easy-coding")).toBeNull();
    expect(screen.queryByTestId("eval-collection-item-hard-coding")).toBeNull();
  });

  it("no longer renders a collection-level Edit button (tasks are edited per-row)", () => {
    render(<EvalManager {...props()} />);
    expect(screen.queryByTestId("eval-edit-collection")).toBeNull();
    expect(screen.getByTestId("eval-new-collection")).toBeInTheDocument();
  });

  it("click expands a collection's tasks; each row has hover Edit/Delete", () => {
    const onEditTask = vi.fn();
    const onDeleteTask = vi.fn();
    render(<EvalManager {...props({ onEditTask, onDeleteTask })} />);
    // Collapsed by default — tasks appear only after clicking the collection.
    expect(screen.queryByTestId("eval-task-row-w")).toBeNull();
    fireEvent.click(screen.getByTestId("eval-collection-item-easy-coding"));
    const row = screen.getByTestId("eval-task-row-w");
    expect(row).toBeInTheDocument();
    // Edit/Delete hidden until hover.
    expect(screen.queryByTestId("eval-task-edit-w")).toBeNull();
    fireEvent.mouseEnter(row);
    fireEvent.click(screen.getByTestId("eval-task-edit-w"));
    expect(onEditTask).toHaveBeenCalledWith("w");
    fireEvent.click(screen.getByTestId("eval-task-delete-w"));
    expect(onDeleteTask).toHaveBeenCalledWith("w");
  });

  it("clicking again collapses the task list", () => {
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-collection-item-easy-coding"));
    expect(screen.getByTestId("eval-task-row-w")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("eval-collection-item-easy-coding"));
    expect(screen.queryByTestId("eval-task-row-w")).toBeNull();
  });

  it("shows the Decoy info popup on hover (Anti-Saturation ⓘ)", () => {
    render(<EvalManager {...props()} />);
    const info = screen.getByTestId("info-decoy");
    fireEvent.mouseEnter(info.parentElement as HTMLElement);
    expect(screen.getByTestId("info-popup-decoy")).toHaveTextContent(/decoy/i);
  });

  it("the tier dropdown no longer offers a 'Custom' option", () => {
    render(<EvalManager {...props()} />);
    const opts = Array.from((screen.getByTestId("eval-tier-dropdown") as HTMLSelectElement).options).map((o) => o.value);
    expect(opts).toEqual(["auto", "easy", "medium", "hard", "extreme"]);
  });

  it("switches to Custom JSON and lists custom collections", async () => {
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByText("◯ Custom JSON"));
    await waitFor(() => expect(select).toHaveBeenCalledWith("my-evals"));
  });

  it("confirms before deleting a custom collection", async () => {
    const remove = vi.fn().mockResolvedValue(undefined);
    useEvalRegistryStore.setState({
      presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
      collections: ["my-evals"],
      selected: "my-evals",
      tasks: sampleTasks,
      init,
      select,
      importFile,
      remove,
    });
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-collection-menu-my-evals"));
    fireEvent.click(screen.getByTestId("eval-collection-delete-my-evals"));
    expect(screen.getByTestId("confirm-dialog")).toBeInTheDocument();
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("confirm-ok"));
    await waitFor(() => expect(remove).toHaveBeenCalledWith("my-evals"));
  });

  it("removes a built-in preset from the list via its ⋯ menu", async () => {
    const hidePreset = vi.fn();
    useEvalRegistryStore.setState({
      presets: [{ id: "hard-coding", label: "Coding (Hard)", domain: "coding", tier: "hard" }],
      collections: [],
      selected: "hard-coding",
      tasks: sampleTasks,
      init,
      select,
      importFile,
      hidePreset,
    });
    render(<EvalManager {...props({ tierSel: "hard", effectiveTier: "hard" })} />);
    fireEvent.click(screen.getByTestId("eval-collection-menu-hard-coding"));
    fireEvent.click(screen.getByTestId("eval-collection-delete-hard-coding"));
    fireEvent.click(screen.getByTestId("confirm-ok"));
    await waitFor(() => expect(hidePreset).toHaveBeenCalledWith("hard-coding"));
  });

  it("always sends the (editable) k AND the tier — k wins in the backend over the tier policy", async () => {
    useParamsStore.setState({ globalParams: { temperature: 0.2 } });
    vi.mocked(runBatchEval).mockResolvedValue({ collection_id: "easy-coding", columns: [] });
    // User edited k to 12 under a Medium tier; both flow to the run.
    render(<EvalManager {...props({ tierSel: "medium", effectiveTier: "medium", recommendedK: 8, k: 12 })} />);
    const runBtn = screen.getByTestId("eval-run-all");
    expect(runBtn).not.toBeDisabled();
    fireEvent.click(runBtn);
    await waitFor(() => {
      expect(runBatchEval).toHaveBeenCalledWith(
        "easy-coding",
        [{ model: "llama3.2:1b", backend: "ollama", is_thinking: false }],
        sampleTasks,
        12, // the editable k, always sent
        8,
        { temperature: 0.2 },
        undefined,
        true, // runNativeFc — PRE-SELECTED by default
        "medium", // tier still flows (for spec.tier)
        undefined, // decoyTools — off by default
        false, // runPromptBased — off by default (Tool-Calling is the default method)
        "standard", // thinkPreset — Standard by default
      );
    });
  });

  it("the k field is always editable and shows the tier's recommended value as a hint", () => {
    render(<EvalManager {...props({ tierSel: "hard", effectiveTier: "hard", recommendedK: 16, k: 16 })} />);
    // Editable input (not a locked span), pre-filled to the recommended value.
    expect(screen.getByTestId("eval-manager-k")).toHaveValue(16);
    expect(screen.queryByTestId("eval-manager-k-locked")).toBeNull();
    expect(screen.getByTestId("eval-k-recommended")).toHaveTextContent("recommended: 16");
    // The user can type a different value.
    fireEvent.change(screen.getByTestId("eval-manager-k"), { target: { value: "9" } });
  });

  it("the Max Steps field is always editable and shows the tier's recommended budget as a hint", () => {
    render(<EvalManager {...props({ tierSel: "hard", effectiveTier: "hard", recommendedSteps: 32, maxSteps: 32 })} />);
    // Editable input, pre-filled to the recommended budget.
    expect(screen.getByTestId("eval-manager-max-steps")).toHaveValue(32);
    expect(screen.getByTestId("eval-steps-recommended")).toHaveTextContent("recommended: 32");
    // The user can override it per run.
    fireEvent.change(screen.getByTestId("eval-manager-max-steps"), { target: { value: "20" } });
  });

  it("shows the HW hint from the backend tier read", () => {
    render(
      <EvalManager
        {...props({ hwTier: { total_memory_bytes: 16 * 1024 ** 3, class: "Mainstream", recommended_tier: "medium" } })}
      />,
    );
    expect(screen.getByTestId("eval-hw-hint")).toHaveTextContent("HW: 16GB RAM · Mainstream · Medium recommended");
  });

  it("flows the decoy budget into the run only when enabled", async () => {
    useParamsStore.setState({ globalParams: {} });
    vi.mocked(runBatchEval).mockResolvedValue({ collection_id: "easy-coding", columns: [] });
    render(<EvalManager {...props({ tierSel: "easy", effectiveTier: "easy", recommendedK: 5, k: 5, decoyEnabled: true, decoyCount: 4 })} />);
    fireEvent.click(screen.getByTestId("eval-run-all"));
    await waitFor(() => {
      expect(runBatchEval).toHaveBeenCalledWith(
        "easy-coding",
        [{ model: "llama3.2:1b", backend: "ollama", is_thinking: false }],
        sampleTasks,
        5, // editable k, always sent
        8,
        {},
        undefined,
        true, // runNativeFc — PRE-SELECTED by default
        "easy",
        4, // decoyTools
        false, // runPromptBased — off by default
        "standard", // thinkPreset — Standard by default
      );
    });
  });
});

describe("EvalManager guided JSON import (format guide → dry-run validate → import)", () => {
  const failingVerdict = {
    ok: false,
    structural_error: null,
    tasks: [
      {
        id: "bad_task",
        reachable: "yes" as const,
        discriminating: true,
        detail: "ok otherwise",
        semantic: ["bad_task: world_state entity 'Z-99' is in neither the prompt nor any other entity's blob"],
        semantic_warnings: [],
      },
    ],
  };

  it("shows the format guide (with a copyable v2 skeleton) BEFORE opening the file picker", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-manager-import"));
    expect(screen.getByTestId("eval-import-guide-skeleton")).toHaveTextContent(/"world_state"/);
    expect(vi.mocked(open)).not.toHaveBeenCalled();
    expect(importFile).not.toHaveBeenCalled();
  });

  it("a failing dry-run blocks the import and names each finding", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/broken.json");
    importFile.mockResolvedValue(failingVerdict);
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-manager-import"));
    fireEvent.click(screen.getByTestId("confirm-ok")); // Continue → choose file
    await waitFor(() => expect(importFile).toHaveBeenCalledWith("/tmp/broken.json"));
    // Blocking popup names the task + defect; the panel carries the same finding.
    expect(screen.getByTestId("eval-import-blocked-findings")).toHaveTextContent("Z-99");
    expect(screen.getByTestId("eval-validation-result")).toHaveTextContent("Problems found");
    expect(screen.getByTestId("eval-validation-semantic-bad_task-0")).toHaveTextContent("Z-99");
  });

  it("an ok-with-warnings verdict imports (no blocking dialog) and shows amber advisories", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/warned.json");
    importFile.mockResolvedValue({
      ok: true,
      structural_error: null,
      tasks: [{ id: "warn_task", reachable: "yes" as const, discriminating: true, detail: "solvable", semantic: [], semantic_warnings: ["warn_task: expected log(...) globs on 'quantize' — checked the prompt, 7 tool names..."] }],
    });
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-manager-import"));
    fireEvent.click(screen.getByTestId("confirm-ok"));
    await waitFor(() => expect(importFile).toHaveBeenCalledWith("/tmp/warned.json"));
    expect(screen.queryByTestId("eval-import-blocked-findings")).toBeNull(); // advisory, not a block
    expect(screen.getByTestId("eval-validation-warning-warn_task-0")).toHaveTextContent("quantize");
  });

  it("a clean dry-run imports without any blocking dialog", async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    vi.mocked(open).mockResolvedValue("/tmp/good.json");
    importFile.mockResolvedValue(null);
    render(<EvalManager {...props()} />);
    fireEvent.click(screen.getByTestId("eval-manager-import"));
    fireEvent.click(screen.getByTestId("confirm-ok"));
    await waitFor(() => expect(importFile).toHaveBeenCalledWith("/tmp/good.json"));
    expect(screen.queryByTestId("eval-import-blocked-findings")).toBeNull();
    expect(screen.queryByTestId("eval-validation-result")).toBeNull();
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../shared/ipc/eval/cliff", async (importActual) => ({
  // Only the IPC calls are stubbed. `CLIFF_CTX_HEADROOM` / `usableCliffTokens` are pure
  // values the panel sizes its slider with — stubbing them would let the ladder-fits-the-
  // window assertions below pass against a fake headroom, testing the mock, not the cap.
  ...(await importActual<typeof import("../../../shared/ipc/eval/cliff")>()),
  runContextCliff: vi.fn(),
  stopContextCliff: vi.fn().mockResolvedValue(undefined),
  getCliffResults: vi.fn().mockResolvedValue({}),
  EVENT_CLIFF_PROGRESS: "cliff-progress",
  // The store subscribes to the fine-grained cliff-step stream too; without this the
  // mock-access throws inside runProbe and the probe aborts before the chart renders.
  EVENT_CLIFF_STEP: "cliff-step",
}));
vi.mock("../../../shared/ipc/eval/registry", () => ({
  getBuiltinCollection: vi.fn(),
  loadCustomCollection: vi.fn(),
}));
vi.mock("../../../shared/ipc/system/inspect", () => ({
  inspectModel: vi.fn(),
  estimateKvCacheBytes: vi.fn(),
}));
vi.mock("../../../shared/ipc/compare/hardware", () => ({ getHardwareSnapshot: vi.fn() }));
vi.mock("../../../shared/ipc/system/vram", () => ({ loadedModels: vi.fn() }));

import { runContextCliff } from "../../../shared/ipc/eval/cliff";
import { getBuiltinCollection } from "../../../shared/ipc/eval/registry";
import { inspectModel, estimateKvCacheBytes } from "../../../shared/ipc/system/inspect";
import { getHardwareSnapshot } from "../../../shared/ipc/compare/hardware";
import { loadedModels } from "../../../shared/ipc/system/vram";
import { CLIFF_CTX_HEADROOM } from "../../../shared/ipc/eval/cliff";
import { ContextCliffPanel } from "../components/ContextCliffPanel";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { useParamsStore } from "../../../shared/state/paramsStore";
import { useEvalRegistryStore } from "../state/evalRegistryStore";

const tasks = [{
  id: "t", category: "single", prompt: "p",
  tools: [{ name: "w", description: "", parameters: { type: "object", properties: {} } }],
  expected: { type: "call", name: "w", args: {} },
}];

// Backend CliffPoint / CliffReport: one rung = (verified depth, worst composite).
const rung = (verified_tokens: number, composite: number | null) => ({
  target_tokens: verified_tokens, verified_tokens, composite, per_depth: [],
});
type Status = { status: "Collapsed"; depth: number } | { status: "NoCliff"; tested: number } | { status: "Broken"; tested: number };
const reportOf = (status: Status, points: ReturnType<typeof rung>[], cliff_tokens: number | null = null) => ({ points, status, cliff_tokens });

const dims = (context_length: number) => ({
  available: true, note: null, template: "", capabilities: [], family: null,
  parameter_size: null, quantization: null, is_base_guess: false, base_reason: null,
  dims: { layers: 0, head_count: 0, head_count_kv: 0, embedding_length: 0, context_length },
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getBuiltinCollection).mockResolvedValue(tasks as never);
  vi.mocked(inspectModel).mockResolvedValue({ dims: null } as never);
  vi.mocked(estimateKvCacheBytes).mockResolvedValue(0 as never);
  // Default: no hardware read + no loaded model → the memory advisory stays hidden, so
  // the existing tests are untouched. The fit-warning test overrides these.
  vi.mocked(getHardwareSnapshot).mockResolvedValue(null as never);
  vi.mocked(loadedModels).mockResolvedValue([] as never);
  useInstalledModelsStore.setState({
    list: [{ name: "m", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "ollama" }],
    status: "ready", error: null, lastRefreshedAt: 1,
  });
  // The probe runs the global header model — select it.
  useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "ollama", size_bytes: 1 }] });
  useParamsStore.setState({ globalParams: {} });
  // Bypass real init (IPC) — seed presets so the panel loads its own dataset.
  useEvalRegistryStore.setState({
    presets: [{ id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" }],
    collections: [],
    init: vi.fn().mockResolvedValue(undefined),
  });
});

describe("ContextCliffPanel", () => {
  it("defaults the padding source to the Corporate Policy preset", () => {
    render(<ContextCliffPanel />);
    expect((screen.getByTestId("cliff-source-select") as HTMLSelectElement).value).toBe("corporate_policy");
  });

  it("lets the user pick the tool-calling method, defaulting to Native FC", () => {
    render(<ContextCliffPanel />);
    // Native is the default active choice on this page…
    expect(screen.getByTestId("cliff-method-native")).toBeEnabled();
    expect(screen.getByTestId("cliff-method-prompt")).toBeEnabled();
  });

  it("disables Native FC on MLX (no native tool API) — prompt-based only", () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "mlx", size_bytes: 1 }] });
    render(<ContextCliffPanel />);
    expect(screen.getByTestId("cliff-method-native")).toBeDisabled();
    expect(screen.getByTestId("cliff-method-prompt")).toBeEnabled();
  });

  it("warns before running when the requested depth won't fit device memory (Ollama)", async () => {
    const GB = 1024 ** 3;
    vi.mocked(inspectModel).mockResolvedValue(dims(8192) as never);
    vi.mocked(estimateKvCacheBytes).mockResolvedValue((20 * GB) as never); // huge KV at depth
    vi.mocked(loadedModels).mockResolvedValue([{ name: "m", size_bytes: 5 * GB, size_vram_bytes: 5 * GB }] as never);
    // 10 GB unified machine can't hold 5 GB weights + 20 GB KV.
    vi.mocked(getHardwareSnapshot).mockResolvedValue({ total_memory_bytes: 10 * GB, gpu: { unified: true } } as never);
    render(<ContextCliffPanel />);
    const warn = await screen.findByTestId("cliff-fit-warning");
    expect(warn).toHaveTextContent(/reduce max tokens/i);
  });

  it("plots the cliff at the model's REAL measured token depth from the backend report", async () => {
    // The backend already padded, swept, verified, and classified — the panel just
    // charts the returned rungs (collapse at the rung verified at ~8300 tokens).
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Collapsed", depth: 8300 }, [rung(120, 1.0), rung(4200, 1.0), rung(8300, 0.5), rung(12400, 0.4), rung(16500, 0.3)], 4200) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));

    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("≈8000 context tokens"));
    expect(screen.getByTestId("cliff-chart")).toBeTruthy();
  });

  it("shows 'Not available' for a rung the backend reported no token count for", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 0 }, [rung(0, 1.0)]) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-panel")).toHaveTextContent("Not available"));
  });

  it("never claims accuracy maintained to a fake '≈0 tokens' when token depth is unreported", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 0 }, [rung(0, 1.0)]) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("not reported"));
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent("≈0");
  });

  it("a cliff whose collapse rung has no measured depth still reads as a cliff (never 'no cliff')", async () => {
    // Baseline holds (1.0), then accuracy collapses on a rung the backend gave no token
    // count for (verified_tokens 0 → null depth). The verdict must still be a CLIFF — the
    // old read-out fell through to a non-cliff "Ran — …" message, contradicting the collapse.
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Collapsed", depth: 0 }, [rung(1000, 1.0), rung(0, 0.2)]) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent(/Cliff detected/i));
    // Invariant: a detected cliff is NEVER reported as healthy.
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent(/maintained/i);
    // And no fabricated precise depth — the collapse rung had no measurement.
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent(/≈\d/);
  });

  it("surfaces a backend error instead of a silent blank chart", async () => {
    vi.mocked(runContextCliff).mockRejectedValue(new Error("server down"));
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-error")).toHaveTextContent("server down"));
  });

  it("reports 'accuracy maintained' when accuracy never collapses", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 5000 }, [rung(1000, 1.0), rung(5000, 1.0)], 5000) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent(/Accuracy maintained up to/));
  });

  it("reports a broken baseline instead of falsely 'maintaining' 0% accuracy", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Broken", tested: 5000 }, [rung(1000, 0.0), rung(5000, 0.0)]) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent(/broken baseline/i));
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent(/maintained/i);
  });

  it("runs the global model + params and forwards the chosen padding source to the backend", async () => {
    useParamsStore.setState({ globalParams: { temperature: 0.2 } });
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 5000 }, [rung(5000, 1.0)], 5000) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    expect(screen.queryByTestId("cliff-model-select")).toBeNull();
    expect(screen.getByTestId("cliff-model")).toHaveTextContent("m");
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(runContextCliff).toHaveBeenCalled());
    const call = vi.mocked(runContextCliff).mock.calls[0];
    expect(call[0]).toBe("m"); // model
    expect(call[1]).toBe("ollama"); // backend
    expect(call[4]).toEqual({ kind: "preset", preset: "corporate_policy" }); // source
    // Global params flow through; the backend pins greedy (temp 0) + num_ctx — not the panel.
    expect(call[7]).toEqual({ temperature: 0.2 });
  });

  it("with 2+ selected Ollama models, a dropdown picks which one the probe runs", async () => {
    useSelectedModelStore.setState({ selectedModels: [
      { name: "m", backend: "ollama", size_bytes: 1 },
      { name: "m2", backend: "ollama", size_bytes: 1 },
    ] });
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 5000 }, [rung(5000, 1.0)], 5000) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    // dropdown appears, listing the selected models; pick the second
    fireEvent.change(screen.getByTestId("cliff-model-select"), { target: { value: "m2" } });
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(runContextCliff).toHaveBeenCalled());
    expect(vi.mocked(runContextCliff).mock.calls[0][0]).toBe("m2");
  });

  // The regression these two guard: the slider used to offer the model's FULL context
  // window, but the backend runs the probe at `maxTokens + CLIFF_CTX_HEADROOM` — so the
  // deepest rung asked for MORE context than the model has, for every model. Ollama
  // answers that by silently clamping and truncating the prompt (deleting the needle,
  // saturating `prompt_eval_count` → a fabricated cliff depth); llama.cpp refuses with
  // "raise the context window", which is impossible. The ladder must fit the window.
  it("caps the Max Tokens slider so the probe's request still fits the context window", async () => {
    vi.mocked(inspectModel).mockResolvedValue(dims(8192) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByTestId("cliff-max-tokens")).toHaveAttribute("max", String(8192 - CLIFF_CTX_HEADROOM)),
    );
  });

  it("defaults Max Tokens to the deepest MEASURABLE depth, not the full window", async () => {
    vi.mocked(inspectModel).mockResolvedValue(dims(32768) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    // Sweep as deep as the model can actually hold — the whole window minus the headroom,
    // not a fixed 16384, and never the full window (that overflows).
    const slider = () => screen.getByTestId("cliff-max-tokens") as HTMLInputElement;
    await waitFor(() => expect(slider().value).toBe(String(32768 - CLIFF_CTX_HEADROOM)));
    // The invariant, stated directly: what the backend will request fits the window.
    expect(Number(slider().value) + CLIFF_CTX_HEADROOM).toBeLessThanOrEqual(32768);
  });

  it("pre-fills model + collection + max tokens + steps from a Matrix request and does NOT auto-run", async () => {
    const { useCliffStore } = await import("../state/cliffStore");
    // The Matrix sets this before navigating to Audit.
    useCliffStore.setState({ request: { model: "m2", backend: "ollama", collectionId: "easy-coding", maxTokens: 8192, steps: 7 } });

    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalledWith("easy-coding"));

    // All four fields land pre-selected (the override model, not the header "m").
    expect((screen.getByTestId("cliff-model-select") as HTMLSelectElement).value).toBe("m2");
    expect((screen.getByTestId("cliff-max-tokens") as HTMLInputElement).value).toBe("8192");
    expect((screen.getByTestId("cliff-test-steps") as HTMLInputElement).value).toBe("7");
    // Request is one-shot.
    expect(useCliffStore.getState().request).toBeNull();
    // GUARDRAIL 1: pre-fill only — the probe never starts on navigation.
    expect(runContextCliff).not.toHaveBeenCalled();
  });

  it("carries the GGUF path from a Matrix re-probe into the llama.cpp probe (modelPath, not empty)", async () => {
    const { useCliffStore } = await import("../state/cliffStore");
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 0 }, [rung(0, 1.0)]) as never);
    useInstalledModelsStore.setState({
      list: [{ name: "m", size_bytes: 1, modified_at: "", family: "", parameter_size: "", quantization: "", backend: "llama_cpp", path: "/w/m.gguf" }],
      status: "ready", error: null, lastRefreshedAt: 1,
    });
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "llama_cpp", size_bytes: 1 }] });
    // A Matrix re-probe now carries the GGUF path (the fix); the override must keep it.
    useCliffStore.setState({ request: { model: "m", backend: "llama_cpp", collectionId: "easy-coding", maxTokens: 16384, steps: 5, path: "/w/m.gguf" } });
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(runContextCliff).toHaveBeenCalled());
    // modelPath is positional arg #9 — must be the real GGUF path, not undefined (which the
    // backend turns into "" → the false "Start llama.cpp with …" WrongModel error).
    expect(vi.mocked(runContextCliff).mock.calls[0][9]).toBe("/w/m.gguf");
  });

  it("explains how Accuracy is calculated via an info button on the column header", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 5000 }, [rung(5000, 1.0)], 5000) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));
    // The Accuracy header carries an ⓘ; hovering reveals the composite-score explanation.
    const info = await screen.findByTestId("info-cliff-accuracy");
    fireEvent.mouseEnter(info.parentElement as HTMLElement);
    expect(screen.getByTestId("info-popup-cliff-accuracy")).toHaveTextContent(/composite tool-call score/i);
  });

  it("reveals the system prompt + padded input + output per step via 'View trace' (collapsed until clicked)", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 4096 }, [
        // Rung 0 is the unpadded baseline; rung 1 carries the injected padding.
        {
          target_tokens: 0, verified_tokens: 343, composite: 1.0, per_depth: [],
          trace: [{ task_id: "t", outputs: [{ depth: 0.0, prompt: "BARE-INSTRUCTION", output: "OUT-0", passed: true }] }],
        } as never,
        {
          target_tokens: 4096, verified_tokens: 4096, composite: 1.0, per_depth: [],
          trace: [{ task_id: "t", outputs: [{ depth: 0.1, prompt: "PADDED-INPUT-MARKER", output: "OUT-1", passed: true }] }],
        } as never,
      ], 4096) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    // Each row offers a per-step trace toggle, but nothing is dumped until the user asks.
    await waitFor(() => expect(screen.getByTestId("cliff-trace-toggle-1")).toBeInTheDocument());
    expect(screen.queryByText("PADDED-INPUT-MARKER")).toBeNull();

    // The padded rung shows the injected context under a "padded input" label.
    fireEvent.click(screen.getByTestId("cliff-trace-toggle-1"));
    const padded = screen.getByTestId("cliff-trace-1");
    expect(padded).toHaveTextContent(/Padded input/i);
    expect(padded).toHaveTextContent("PADDED-INPUT-MARKER");
    expect(padded).toHaveTextContent("OUT-1");
    // The system prompt (boilerplate + tool JSON) is intentionally not shown.
    expect(padded).not.toHaveTextContent(/System prompt/i);

    // The baseline rung is labelled honestly as unpadded — no "padded input" claim there.
    fireEvent.click(screen.getByTestId("cliff-trace-toggle-0"));
    const base = screen.getByTestId("cliff-trace-0");
    expect(base).toHaveTextContent(/Unpadded baseline/i);
    expect(base).toHaveTextContent(/Input \(no padding\)/i);
    expect(base).toHaveTextContent("BARE-INSTRUCTION");
    expect(base).not.toHaveTextContent(/Padded input/i);
  });

  it("re-pre-fills when a NEW request arrives after mount (always-mounted Audit panel)", async () => {
    const { useCliffStore } = await import("../state/cliffStore");
    render(<ContextCliffPanel />);
    await waitFor(() => expect(getBuiltinCollection).toHaveBeenCalled());
    // No request at mount → header model. Then the user clicks Run probe on the Matrix:
    act(() => useCliffStore.setState({ request: { model: "m3", backend: "ollama", collectionId: "easy-coding", maxTokens: 4096, steps: 3 } }));
    await waitFor(() => expect((screen.getByTestId("cliff-model-select") as HTMLSelectElement).value).toBe("m3"));
    expect((screen.getByTestId("cliff-test-steps") as HTMLInputElement).value).toBe("3");
    expect(useCliffStore.getState().request).toBeNull(); // consumed
  });
});

describe("thinking budget control", () => {
  it("stays hidden for a non-thinking model and dispatches the flat budget", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 0 }, [rung(0, 1.0)]) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    // "m" matches no reasoning marker → no thinking row on this probe.
    expect(screen.queryByTestId("cliff-thinking")).toBeNull();
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(runContextCliff).toHaveBeenCalled());
    const args = vi.mocked(runContextCliff).mock.calls[0];
    expect(args[11]).toBe(false); // isThinking
  });

  it("shows the preset for a thinking model (default Standard) and sends the choice with the run", async () => {
    useSelectedModelStore.setState({ selectedModels: [{ name: "qwen3.5-9b", backend: "ollama", size_bytes: 1 }] });
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 0 }, [rung(0, 1.0)]) as never);
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());

    // The control is visible with Standard active, and the budget label shows the
    // depth-banded scratchpad for the CURRENT Max Tokens (16384 → hard band).
    expect(screen.getByTestId("cliff-thinking")).toBeTruthy();
    expect(screen.getByTestId("cliff-thinking-budget")).toHaveTextContent(`+${(10240).toLocaleString()} tokens`);

    // Pick Deep → the label follows the same band at the bigger preset…
    fireEvent.click(screen.getByTestId("cliff-thinking-deep"));
    expect(screen.getByTestId("cliff-thinking-budget")).toHaveTextContent(`+${(20480).toLocaleString()} tokens`);

    // …and the run carries the thinking flag + chosen preset to the backend.
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(runContextCliff).toHaveBeenCalled());
    const args = vi.mocked(runContextCliff).mock.calls[0];
    expect(args[11]).toBe(true); // isThinking
    expect(args[12]).toBe("deep"); // thinkPreset
  });
});

describe("per-task breakdown (by_task)", () => {
  it("names the failing tasks under a rung's accuracy, with the task count", async () => {
    const collapsed = {
      ...rung(8845, 0.733),
      passed: 11, trials: 15,
      by_task: [
        { task_id: "md_co_trace_root_cause", passed: 3, trials: 3 },
        { task_id: "md_co_secret_rotation_by_svc", passed: 0, trials: 3 },
        { task_id: "md_co_pii_log_redaction", passed: 2, trials: 3 },
      ],
    };
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Collapsed", depth: 8845 }, [rung(700, 1.0), collapsed], 700) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    // The failing tasks — and ONLY those — are named on the collapse rung.
    await waitFor(() => expect(screen.getByTestId("cliff-by-task-1")).toBeTruthy());
    const line = screen.getByTestId("cliff-by-task-1");
    expect(line).toHaveTextContent("md_co_secret_rotation_by_svc 0/3");
    expect(line).toHaveTextContent("md_co_pii_log_redaction 2/3");
    expect(line).not.toHaveTextContent("md_co_trace_root_cause");
    // The clean rung shows no failure line.
    expect(screen.queryByTestId("cliff-by-task-0")).toBeNull();
  });
});

describe("concentration low-confidence labeling", () => {
  it("carries the one-task clause on the read-out when the backend flags concentration", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf(
        {
          status: "Collapsed",
          depth: 8845,
          concentration: {
            task_id: "md_co_secret_rotation_by_svc",
            task_failures: 3,
            total_failures: 4,
            p_value_milli: 44,
            holds_without: true,
          },
        } as never,
        [rung(700, 1.0), rung(8845, 0.733)],
        700,
      ) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("low confidence"));
    const read = screen.getByTestId("cliff-read");
    expect(read).toHaveTextContent("3 of 4 failures from one task (md_co_secret_rotation_by_svc");
    expect(read).toHaveTextContent("depth-general collapse not established");
  });

  it("renders a plain collapse unchanged when no concentration was flagged", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Collapsed", depth: 8300 }, [rung(120, 1.0), rung(8300, 0.5)], 4200) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));
    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("≈8000 context tokens"));
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent("low confidence");
  });
});

describe("deliberation headroom", () => {
  it("renders BudgetLimited as a budget outcome, never as a collapse, with cap and amber flags", async () => {
    const deep = {
      ...rung(8845, 0.733),
      passed: 11, trials: 15, max_output: 256,
      by_task: [
        { task_id: "ok_task", passed: 3, trials: 3, failed_cap_hits: 0, min_pass_headroom_milli: 500 },
        { task_id: "near_cap_task", passed: 3, trials: 3, failed_cap_hits: 0, min_pass_headroom_milli: 62 },
        { task_id: "starved_task", passed: 0, trials: 3, failed_cap_hits: 3, min_pass_headroom_milli: null },
        { task_id: "starved_too", passed: 2, trials: 3, failed_cap_hits: 1, min_pass_headroom_milli: 400 },
      ],
    };
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "BudgetLimited", depth: 8845, cap: 256 } as never, [rung(700, 1.0), deep], null) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    // Read-out: the budget outcome, with the disambiguating-probe instruction.
    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("Budget-limited"));
    expect(screen.getByTestId("cliff-read")).toHaveTextContent("256-token output cap");
    expect(screen.getByTestId("cliff-read")).not.toHaveTextContent("context tokens —"); // not a collapse read-out
    // Failing line marks the cap deaths; amber line names the near-cap passer only.
    const failLine = screen.getByTestId("cliff-by-task-1");
    expect(failLine).toHaveTextContent("starved_task 0/3 (3 died at cap)");
    const amber = screen.getByTestId("cliff-near-cap-1");
    expect(amber).toHaveTextContent("near_cap_task (62‰ headroom)");
    expect(amber).not.toHaveTextContent("ok_task");
  });
});

describe("three-bucket aggregate", () => {
  it("renders the triple — never a rate — on a cap-affected rung", async () => {
    const deep = {
      ...rung(9062, null), // composite blanked by the engine on cap-affected rungs
      passed: 5, trials: 9, max_output: 256, cap_deaths: 4,
      by_task: [
        { task_id: "secret", passed: 1, trials: 3, failed_cap_hits: 2, min_pass_headroom_milli: 0 },
        { task_id: "rollback", passed: 3, trials: 3, failed_cap_hits: 0, min_pass_headroom_milli: 200 },
        { task_id: "flat", passed: 1, trials: 3, failed_cap_hits: 2, min_pass_headroom_milli: null },
      ],
    };
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 9062 }, [rung(700, 1.0), deep], null) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("content-only claim"));
    // The rung row shows the triple, not a percentage.
    expect(screen.getByText("5 passed · 0 failed · 4 died-at-cap")).toBeTruthy();
  });
});

describe("every verdict state reaches the UI", () => {
  it("cap-affected rung: Budget chip (never 'Error'), and the read-out stays authoritative when the baseline itself has a cap death", async () => {
    // Baseline composite is null (cap-affected) — the frontend classifier would say
    // "no-baseline"; the backend status must drive the read-out instead.
    const base = { ...rung(732, null), passed: 2, trials: 3, max_output: 256, cap_deaths: 1,
      by_task: [{ task_id: "flat", passed: 0, trials: 1, failed_cap_hits: 1, min_pass_headroom_milli: null },
                { task_id: "a", passed: 1, trials: 1, failed_cap_hits: 0, min_pass_headroom_milli: 300 },
                { task_id: "b", passed: 1, trials: 1, failed_cap_hits: 0, min_pass_headroom_milli: 400 }] };
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 9062 }, [base, rung(9062, 1.0)], null) as never,
    );
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).not.toBeDisabled());
    fireEvent.click(screen.getByTestId("cliff-run"));

    await waitFor(() => expect(screen.getByTestId("cliff-read")).toHaveTextContent("Accuracy maintained up to ≈9000 tokens"));
    expect(screen.getByTestId("cliff-read")).toHaveTextContent("content-only claim");
    expect(screen.getByTestId("cliff-budget-chip-0")).toHaveTextContent("Budget");
    expect(screen.queryByText("Error")).toBeNull();
  });
});

vi.mock("../../../shared/ipc/models/llama_start", () => ({
  // Default: no running server — pre-existing llama tests keep their old world; the
  // running-window tests override per-case.
  llamaRunningWindow: vi.fn().mockResolvedValue(null),
}));

describe("llama.cpp running-window cap", () => {
  it("caps the slider at the RUNNING server's window and names both levers", async () => {
    const { llamaRunningWindow } = await import("../../../shared/ipc/models/llama_start");
    vi.mocked(llamaRunningWindow).mockResolvedValue({ path: "/w/m.gguf", ctx: 12288 });
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "llama_cpp", size_bytes: 1, path: "/w/m.gguf" } as never] });
    render(<ContextCliffPanel />);

    // usable(12288) = 12288 − 2048 = 10240 — the server window binds, not the 65536 fallback.
    await waitFor(() => expect((screen.getByTestId("cliff-max-tokens") as HTMLInputElement).max).toBe("10240"));
    const hint = screen.getByTestId("cliff-server-window-hint");
    expect(hint).toHaveTextContent("12,288 tokens");
    expect(hint).toHaveTextContent("raise “Context window”");
  });

  it("says plainly when no llama-server is running", async () => {
    const { llamaRunningWindow } = await import("../../../shared/ipc/models/llama_start");
    vi.mocked(llamaRunningWindow).mockResolvedValue(null);
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "llama_cpp", size_bytes: 1 }] });
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-no-server-hint")).toBeTruthy());
  });

  it("shows NEITHER hint while the window probe is still in flight (no false 'no server' flash)", async () => {
    const { llamaRunningWindow } = await import("../../../shared/ipc/models/llama_start");
    // A probe that never resolves within the test — the panel must say nothing, not
    // flash "No running llama-server detected" at a user whose server is fine.
    vi.mocked(llamaRunningWindow).mockReturnValue(new Promise(() => {}));
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "llama_cpp", size_bytes: 1 }] });
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-run")).toBeTruthy());
    expect(screen.queryByTestId("cliff-no-server-hint")).toBeNull();
    expect(screen.queryByTestId("cliff-server-window-hint")).toBeNull();
  });

  it("surfaces an IPC failure as the no-server hint (start the server either way)", async () => {
    const { llamaRunningWindow } = await import("../../../shared/ipc/models/llama_start");
    vi.mocked(llamaRunningWindow).mockRejectedValue(new Error("connection refused"));
    useSelectedModelStore.setState({ selectedModels: [{ name: "m", backend: "llama_cpp", size_bytes: 1 }] });
    render(<ContextCliffPanel />);
    await waitFor(() => expect(screen.getByTestId("cliff-no-server-hint")).toBeTruthy());
  });
});

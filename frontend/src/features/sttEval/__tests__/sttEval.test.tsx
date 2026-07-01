import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { EvalReportTable } from "../components/EvalReportTable";
import { EvalVerdictTable } from "../components/EvalVerdictTable";
import { SttEvalPanel } from "../components/SttEvalPanel";
import { SttEvalEditor } from "../components/SttEvalEditor";
import type { SttReportRow, SttModelVerdict } from "../../../shared/ipc/stt/eval";

const wer = (over: Partial<SttReportRow["wer"] & object> = {}) => ({
  wer: 0.25, weighted_wer: 0.42, adjusted_wer: 0.25, substitutions: 1, insertions: 0,
  deletions: 0, ref_words: 4, critical_token_accuracy: 0.5, misreads: [], ...over,
});
const row = (id: string, w: SttReportRow["wer"]): SttReportRow => ({
  task_id: id, model: "whisper-base.en", rtf: 2, repeat_rate: 0, silence_rate: 0, confidence: 0.9, wer: w,
});

beforeEach(() => invokeMock.mockReset());

describe("EvalReportTable", () => {
  it("shows WER for a referenced row and N/A for a reference-less one", () => {
    render(<EvalReportTable rows={[row("t1", wer()), row("t2", null)]} />);
    expect(screen.getByTestId("wer-t1").textContent).toBe("25%");
    expect(screen.getByTestId("wer-t2").textContent).toBe("N/A"); // no reference → never fabricated
  });

  it("lists misreads when present", () => {
    render(<EvalReportTable rows={[row("t3", wer({ misreads: [{ reference: "ruben", heard: "reuben", probability: 0.97 }] }))]} />);
    expect(screen.getByTestId("misreads-t3").textContent).toContain("ruben→reuben");
  });
});

describe("EvalVerdictTable", () => {
  const verdict = (status: string, blocking: string[], conditions: string[]): SttModelVerdict => ({
    model: "whisper-base.en",
    verdict: { status: status as never, blocking, conditions },
    rtf: 2, wer: 0.25, weighted_wer: 0.42, repeat_rate: 0, silence_rate: 0, confidence: 0.9, memory: null,
  });

  it("renders a blocking verdict with its reason", () => {
    render(<EvalVerdictTable verdicts={[verdict("not_ready", ["weighted WER 42% > 5% allowed"], [])]} />);
    expect(screen.getByTestId("stt-verdict-status-whisper-base.en").textContent).toBe("Not ready");
    expect(screen.getByText("weighted WER 42% > 5% allowed")).toBeInTheDocument();
  });

  it("renders the accuracy-unverified condition", () => {
    render(<EvalVerdictTable verdicts={[verdict("conditional", [], ["accuracy unverified (no reference text)"])]} />);
    expect(screen.getByTestId("stt-verdict-status-whisper-base.en").textContent).toBe("Conditional");
    expect(screen.getByText("accuracy unverified (no reference text)")).toBeInTheDocument();
  });
});

describe("SttEvalPanel", () => {
  it("loads specs + profiles and runs an eval into the report table", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_stt_evals") return ["my-eval"];
      if (cmd === "list_stt_readiness_profiles") return [{ id: "high-accuracy-legal", name: "High accuracy (legal/financial)", min_rtf: null, max_wer: 0.05, max_repeat_rate: 0.02, max_silence_rate: 0.01, min_confidence: 0.85, require_vram_fit: false }];
      if (cmd === "run_stt_eval") return { rows: [row("t1", wer())] };
      return undefined;
    });
    render(<SttEvalPanel />);
    await waitFor(() => expect(screen.getByTestId("stt-eval-spec")).toBeInTheDocument());
    await act(async () => {
      fireEvent.click(screen.getByTestId("stt-eval-run"));
    });
    expect(invokeMock).toHaveBeenCalledWith("run_stt_eval", { spec: "my-eval" });
    expect(await screen.findByTestId("stt-eval-row-t1")).toBeInTheDocument();
  });

  it("shows the empty state when there are no specs", async () => {
    invokeMock.mockResolvedValue([]);
    render(<SttEvalPanel />);
    expect(await screen.findByTestId("stt-eval-no-specs")).toBeInTheDocument();
  });

  it("'+ New spec' opens the editor", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_transcripts") return [];
      return [];
    });
    render(<SttEvalPanel />);
    await act(async () => {
      fireEvent.click(await screen.findByTestId("stt-eval-new"));
    });
    expect(screen.getByTestId("stt-eval-editor")).toBeInTheDocument();
  });
});

describe("SttEvalEditor", () => {
  it("generate-starter builds a self-referenced task per transcript and saves it", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_transcripts") return [{ id: "clip-1", model: "m", text: "hello world" }];
      return undefined;
    });
    const onSaved = vi.fn();
    render(<SttEvalEditor onSaved={onSaved} onCancel={() => {}} />);
    await waitFor(() => expect(screen.getByTestId("stt-eval-starter")).toBeEnabled());
    await act(async () => {
      fireEvent.click(screen.getByTestId("stt-eval-starter")); // flush setRows + setName before reading
    });
    expect(screen.getByTestId("stt-eval-ref-0")).toHaveValue("hello world"); // reference prefilled
    expect(screen.getByTestId("stt-eval-name")).toHaveValue("starter");
    await act(async () => {
      fireEvent.click(screen.getByTestId("stt-eval-save"));
    });
    const call = invokeMock.mock.calls.find((c) => c[0] === "save_stt_eval");
    expect(call?.[1]).toEqual({
      name: "starter",
      spec: { tasks: [{ id: "clip-1", reference: "hello world", critical_tokens: [] }] },
    });
    expect(onSaved).toHaveBeenCalledWith("starter");
  });

  it("parses critical tokens and turns a blank reference into null", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "list_transcripts") return [{ id: "clip-1", model: "m", text: "hi" }];
      return undefined;
    });
    render(<SttEvalEditor initialName="legal-test" onSaved={vi.fn()} onCancel={() => {}} />);
    await waitFor(() => expect(screen.getByTestId("stt-eval-add")).toBeEnabled());
    fireEvent.click(screen.getByTestId("stt-eval-add"));
    fireEvent.change(screen.getByTestId("stt-eval-ref-0"), { target: { value: "   " } });
    fireEvent.change(screen.getByTestId("stt-eval-crit-0"), { target: { value: "$100, ruben" } });
    await act(async () => {
      fireEvent.click(screen.getByTestId("stt-eval-save"));
    });
    const call = invokeMock.mock.calls.find((c) => c[0] === "save_stt_eval");
    expect(call?.[1]).toEqual({
      name: "legal-test",
      spec: { tasks: [{ id: "clip-1", reference: null, critical_tokens: ["$100", "ruben"] }] },
    });
  });
});

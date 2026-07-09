import { useEffect } from "react";
import type { TaskOutcome, TrajectoryStep } from "../../../shared/ipc/eval/batch";
import type { ToolTask } from "../../../shared/ipc/eval/registry";
import { buildRunInput, buildRunOutput, type RunOutputRun } from "./runIo";

export type RunIoMode = "input" | "output";

interface RunIoModalProps {
  task: ToolTask;
  outcome: TaskOutcome | undefined;
  /// The trajectory steps for THIS run. For an agentic task the caller passes only the
  /// selected run's steps (filtered by `run_index`); for a single-turn task this is empty
  /// and the output comes from `outcome.trace`.
  steps: TrajectoryStep[];
  /// Header label naming which run this is (e.g. "RUN 3 OF 5" or "Single-turn run").
  title: string;
  /// The run's decoy-tool budget (scoreboard run shape) — surfaced in the Input view
  /// so a reconstructed agentic prompt admits the tools the model also saw at run time.
  decoys?: number;
  mode: RunIoMode;
  setMode: (mode: RunIoMode) => void;
  onClose: () => void;
}

/// A focused drill-down for ONE run in the Evaluator's trace: the prompt the model was
/// given (Input) and the raw response it produced (Output), switchable via a header
/// toggle. All view-model logic lives in `runIo.ts`; this file only renders it (incl.
/// the explicit "no response" states for an empty / errored / not-yet-run output).
export function RunIoModal({ task, outcome, steps, title, decoys, mode, setMode, onClose }: RunIoModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      role="presentation"
      onClick={onClose}
      className="fixed inset-0 z-40 bg-black/40 flex items-center justify-center p-4"
      data-testid="run-io-modal"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`${title} ${mode} for ${task.id}`}
        onClick={(e) => e.stopPropagation()}
        style={cardStyle}
      >
        {/* Header — run title, task id, Input/Output toggle, close. */}
        <div style={headerStyle}>
          <span style={{ fontSize: 13, fontWeight: 700, color: "#0f172a", fontFamily: "Inter, sans-serif" }} data-testid="run-io-title">{title}</span>
          <span style={taskChipStyle}>{task.id}</span>
          <div style={{ display: "inline-flex", gap: 6, marginLeft: 12 }}>
            {(["input", "output"] as RunIoMode[]).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => setMode(m)}
                data-testid={`run-io-tab-${m}`}
                style={tabStyle(mode === m)}
              >
                {m === "input" ? "Input" : "Output"}
              </button>
            ))}
          </div>
          <button type="button" onClick={onClose} data-testid="run-io-close" style={closeBtnStyle} aria-label="Close" title="Close (Esc)">
            ✕
          </button>
        </div>

        {/* Body */}
        <div style={bodyStyle}>
          {mode === "input" ? (
            <InputView task={task} outcome={outcome} decoys={decoys} steps={steps} />
          ) : (
            <OutputView outcome={outcome} steps={steps} />
          )}
        </div>
      </div>
    </div>
  );
}

function InputView({
  task,
  outcome,
  decoys,
  steps,
}: {
  task: ToolTask;
  outcome: TaskOutcome | undefined;
  decoys?: number;
  steps: TrajectoryStep[];
}) {
  const input = buildRunInput(task, outcome, decoys, steps);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }} data-testid="run-io-input">
      {input.note && (
        <div style={noteStyle} data-testid="run-io-input-approx">
          {input.note}
        </div>
      )}
      <section>
        <div style={labelStyle}>System Message</div>
        <pre style={codeBlockStyle}>{input.system}</pre>
      </section>
      <section>
        <div style={labelStyle}>User Prompt</div>
        <pre style={codeBlockStyle}>{input.user}</pre>
      </section>
    </div>
  );
}

function OutputView({ outcome, steps }: { outcome: TaskOutcome | undefined; steps: TrajectoryStep[] }) {
  const out = buildRunOutput(outcome, steps);

  if (out.state === "not_run") {
    return (
      <div style={emptyStateStyle} data-testid="run-io-not-run">
        No model response is cached for this run. Run the batch to capture it.
      </div>
    );
  }
  if (out.state === "error") {
    return (
      <div style={errorCardStyle} data-testid="run-io-error">
        <div style={{ fontWeight: 700, marginBottom: 4 }}>No response — the run errored.</div>
        <pre style={{ ...codeBlockStyle, color: "#991b1b" }}>{out.message}</pre>
      </div>
    );
  }
  if (out.state === "empty") {
    return (
      <div style={warnCardStyle} data-testid="run-io-empty">
        {out.reason}
      </div>
    );
  }
  if (out.state === "single") {
    return (
      <div data-testid="run-io-output">
        <div style={labelStyle}>Model Output (raw)</div>
        <pre style={codeBlockStyle}>{out.output}</pre>
      </div>
    );
  }
  // Agentic. The trace opens this scoped to ONE run, so render that run's turns
  // directly; only wrap in per-run sections if more than one run is present.
  const turns = (run: RunOutputRun) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {run.steps.map((s) => (
        <div key={`${s.run_index}-${s.step_index}`}>
          <div style={turnLabelStyle}>TURN {s.step_index + 1}</div>
          <pre style={codeBlockStyle}>{s.raw_output.trim() === "" ? "(empty output)" : s.raw_output}</pre>
          {s.injection && (
            <pre style={{ ...codeBlockStyle, color: "#2563eb", marginTop: 4 }}>↳ sandbox: {s.injection}</pre>
          )}
        </div>
      ))}
    </div>
  );

  if (out.runs.length === 1) {
    return (
      <div data-testid="run-io-output">{turns(out.runs[0])}</div>
    );
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }} data-testid="run-io-output">
      {out.runs.map((run, i) => (
        <div key={run.runIndex} style={runSectionStyle}>
          <div style={runHeaderStyle}>
            <span style={runTitleStyle}>
              RUN {i + 1} OF {out.runs.length}
            </span>
            <span style={run.passed ? passChipStyle : failChipStyle}>{run.passed ? "PASS" : "FAIL"}</span>
          </div>
          <div style={{ marginTop: 10 }}>{turns(run)}</div>
        </div>
      ))}
    </div>
  );
}

// ── Styles ──────────────────────────────────────────────────────────────────

const cardStyle: React.CSSProperties = {
  background: "#ffffff",
  borderRadius: 16,
  boxShadow: "0 10px 40px rgba(0,0,0,0.18)",
  border: "1px solid #e2e8f0",
  width: "min(880px, 94vw)",
  maxHeight: "68vh",
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
};

const headerStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "14px 18px",
  borderBottom: "1px solid #e2e8f0",
  background: "#fafafa",
};

const taskChipStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#475569",
  background: "#f1f5f9",
  padding: "2px 8px",
  borderRadius: 6,
  fontFamily: "'JetBrains Mono', monospace",
  fontWeight: 600,
};

const tabStyle = (active: boolean): React.CSSProperties => ({
  padding: "4px 12px",
  borderRadius: 8,
  fontSize: 12,
  fontFamily: "Inter, sans-serif",
  cursor: "pointer",
  border: `1px solid ${active ? "#bfdbfe" : "#e2e8f0"}`,
  background: active ? "#eff6ff" : "#ffffff",
  color: active ? "#2563eb" : "#475569",
  fontWeight: active ? 600 : 500,
});

const closeBtnStyle: React.CSSProperties = {
  marginLeft: "auto",
  background: "transparent",
  border: "none",
  cursor: "pointer",
  fontSize: 16,
  color: "#64748b",
  padding: 4,
  lineHeight: 1,
};

const bodyStyle: React.CSSProperties = {
  padding: 18,
  overflowY: "auto",
};

const labelStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 800,
  color: "#64748b",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
  fontFamily: "Inter, sans-serif",
  marginBottom: 6,
};

const codeBlockStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 12,
  lineHeight: 1.6,
  color: "#334155",
  fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  background: "#f8fafc",
  border: "1px solid #e2e8f0",
  borderRadius: 8,
  padding: "10px 12px",
};

const noteStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#92400e",
  background: "#fffbeb",
  border: "1px solid #fde68a",
  borderRadius: 8,
  padding: "8px 12px",
  fontFamily: "Inter, sans-serif",
};

const emptyStateStyle: React.CSSProperties = {
  textAlign: "center",
  color: "#64748b",
  fontSize: 13,
  fontFamily: "Inter, sans-serif",
  padding: "28px 12px",
};

const warnCardStyle: React.CSSProperties = {
  color: "#92400e",
  background: "#fffbeb",
  border: "1px solid #fde68a",
  borderRadius: 8,
  padding: "14px 16px",
  fontSize: 13,
  fontFamily: "Inter, sans-serif",
};

const errorCardStyle: React.CSSProperties = {
  color: "#991b1b",
  background: "#fef2f2",
  border: "1px solid #fecaca",
  borderRadius: 8,
  padding: "14px 16px",
  fontSize: 13,
  fontFamily: "Inter, sans-serif",
};

const runSectionStyle: React.CSSProperties = {
  border: "1px solid #e2e8f0",
  borderRadius: 12,
  padding: "12px 14px",
  background: "#ffffff",
};

const runHeaderStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 10,
};

const runTitleStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 800,
  color: "#475569",
  fontFamily: "Inter, sans-serif",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const turnLabelStyle: React.CSSProperties = {
  fontSize: 10,
  fontWeight: 700,
  color: "#94a3b8",
  fontFamily: "Inter, sans-serif",
  textTransform: "uppercase",
  letterSpacing: "0.04em",
  marginBottom: 4,
};

const chipBase: React.CSSProperties = {
  fontSize: 10,
  fontWeight: 700,
  padding: "2px 8px",
  borderRadius: 4,
  fontFamily: "Inter, sans-serif",
  letterSpacing: "0.03em",
};

const passChipStyle: React.CSSProperties = { ...chipBase, background: "#f0fdf4", color: "#166534", border: "1px solid #bbf7d0" };
const failChipStyle: React.CSSProperties = { ...chipBase, background: "#fef2f2", color: "#991b1b", border: "1px solid #fecaca" };

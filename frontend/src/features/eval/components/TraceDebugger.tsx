import { useState, useEffect } from "react";
import { useBatchStore, cellKey } from "../state/batchStore";
import { useEvalRegistryStore } from "../state/evalRegistryStore";
import { traceDiag } from "../verdict";
import { ConfigPhase } from "./pipeline/ConfigPhase";
import { SystemMessagePhase } from "./pipeline/SystemMessagePhase";
import { VerifyPhase } from "./pipeline/VerifyPhase";
import { InfoButton } from "../../../shared/ui/InfoButton";
import { Spinner } from "../../../shared/ui/Spinner";
import { TOOL_HELP } from "../help";
import { agenticSystemPreview } from "../agenticPrompt";
import { RunIoModal, type RunIoMode } from "./RunIoModal";
import { EnvironmentReplayPanel, hasEnvReplay } from "./replay/EnvironmentReplayPanel";
import { isStrictPass, type TrajectoryStep, type AgenticReport } from "../../../shared/ipc/eval/batch";
import { cacheReuse } from "../../../shared/format/cache";

interface TraceDebuggerProps {
  model: string;
  taskId: string | null;
  setTaskId: (id: string | null) => void;
  /// The active run's decoy-tool budget — passed through to the per-run Input drill-down
  /// so a reconstructed agentic prompt admits the decoy tools the model also saw.
  decoys?: number;
  /// Which pass's trajectory to show — controlled by the parent (the Simulator click), so the
  /// Evaluator has no toggle of its own.
  tracePass: "prompt" | "native";
  /// The run's configured Pass^k — the denominator for the "RUN n OF N" headers while the
  /// batch is still streaming (before the terminal report lands carries the intended total).
  k?: number;
}

type TabType = "config" | "prompt" | "trace" | "matcher";

const turnHeaderTitleStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 800,
  color: "#475569",
  fontFamily: "Inter, sans-serif",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const GearIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
    <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
);

const ErrorIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
  </svg>
);

const CheckIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
  </svg>
);

const LoopIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 1121.21 7.89M9 11l3-3 3 3" />
  </svg>
);

const StopIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
  </svg>
);

const FlagIcon = () => (
  <svg style={{ width: 14, height: 14 }} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
    <path strokeLinecap="round" strokeLinejoin="round" d="M3 21v-4m0 0V5a2 2 0 012-2h6.5l1 1H21l-3 6 3 6h-8.5l-1-1H5a2 2 0 00-2 2zm9-13.5V9" />
  </svg>
);

const getStepIcon = (kind: string, isError: boolean): React.ReactNode => {
  if (kind === "tool_call") return <GearIcon />;
  if (kind === "tool_error" || kind === "schema_error" || kind === "malformed_json" || kind === "turn_timeout" || kind === "foreign_dialect" || kind === "empty_output" || kind === "truncated" || kind === "reasoning_overrun") return <ErrorIcon />;
  if (kind === "infinite_loop") return <LoopIcon />;
  if (kind === "hallucinated_completion" || kind === "forbidden_call" || kind === "reported_in_prose") return <StopIcon />;
  if (kind === "end_state_reached") return <FlagIcon />;
  return isError ? <ErrorIcon /> : <CheckIcon />;
};

/// Step kinds that are FAILURES — they render red, never the green "success" card.
/// `turn_timeout` and `forbidden_call` are terminal failures; omitting them painted a
/// stalled/trapped turn as a green "Model Output Success".
export const isErrorKind = (kind: string): boolean =>
  kind === "tool_error" ||
  kind === "unknown_tool" ||
  kind === "schema_error" ||
  kind === "malformed_json" ||
  kind === "hallucinated_completion" ||
  kind === "infinite_loop" ||
  kind === "forbidden_call" ||
  kind === "turn_timeout" ||
  kind === "reported_in_prose" ||
  kind === "foreign_dialect" ||
  kind === "empty_output" ||
  kind === "truncated" ||
  kind === "reasoning_overrun";

export const getStepTitle = (kind: string, isError: boolean) => {
  if (kind === "tool_call") return "Model Outputs Tool Call";
  if (kind === "tool_error") return "Injected Tool Fault (Driver B)";
  if (kind === "unknown_tool") return "Unknown Tool Triggered";
  if (kind === "schema_error") return "Schema Validation Error (Driver D)";
  if (kind === "malformed_json") return "Malformed JSON Generation";
  if (kind === "infinite_loop") return "Execution Loop Capped";
  if (kind === "hallucinated_completion") return "Hallucinated Stop Word";
  if (kind === "forbidden_call") return "Forbidden Action (Trap Sprung)";
  if (kind === "turn_timeout") return "Turn Timeout (Stalled Model)";
  if (kind === "reported_in_prose") return "Reported In Prose (Wrong Channel)";
  if (kind === "foreign_dialect") return "Foreign Tool Dialect (Unparseable)";
  if (kind === "empty_output") return "Empty Output (No Usable Response)";
  if (kind === "truncated") return "Output Truncated — Context Window Full (Hardware Limit)";
  if (kind === "reasoning_overrun") return "Reasoning Overrun — Thinking Budget Maxed (Setting)";
  if (kind === "end_state_reached") return "End State Verification";
  return isError ? "Execution Failure" : "Model Output Success";
};

/// The failing-run verdict header, derived from the report's actual `top_error` —
/// NOT a hardcoded "sequence violation" (which mislabels malformed/hallucinated/
/// timeout/forbidden runs). The `(Reason: …)` is folded into the title so the header
/// itself names the real cause.
export const verdictLabel = (topError: string): { title: string; detail: string } => {
  switch (topError) {
    case "malformed_json":
      return { title: "MALFORMED JSON", detail: "The model emitted broken JSON where a tool call was expected." };
    case "malformed_schema":
      return { title: "SCHEMA ERRORS UNRECOVERED", detail: "The model's calls failed schema validation and the recovery budget ran out." };
    case "hallucinated":
      return { title: "HALLUCINATED COMPLETION", detail: "The model claimed completion without satisfying the required checkpoints." };
    case "infinite_loop":
      return { title: "STEP BUDGET EXCEEDED", detail: "The model never reached the end state within the step cap (looping)." };
    case "forbidden_call":
      return { title: "FORBIDDEN ACTION", detail: "The model invoked a must_not_call trap — terminal the moment it fired." };
    case "turn_timeout":
      return { title: "TURN TIMEOUT", detail: "A model turn exceeded the per-step wall-clock budget (a stalled model)." };
    case "foreign_dialect":
      return {
        title: "FOREIGN TOOL DIALECT",
        detail:
          "The model emitted an unparseable non-JSON tool dialect (a mis-built model's channel-token soup) — a template/dialect artifact, not a capability failure. Not salvaged: a real deployment couldn't read it either.",
      };
    case "empty_output":
      return {
        title: "EMPTY OUTPUT",
        detail:
          "The model produced no usable output (empty / whitespace / a lone punctuation char before its stop token) — a generation/template artifact, not a hallucinated completion. Often a model that doesn't engage the prompt-based tool format; try Measure native tool-calling.",
      };
    case "reported_in_prose":
      return {
        title: "REPORTED IN PROSE",
        detail: "Correct content, but the model answered in plain text instead of calling the required tool — a wrong-channel failure, not a hallucination.",
      };
    case "truncated":
      return {
        title: "OUTPUT TRUNCATED AT TOKEN CAP",
        detail:
          "The turn was cut off at the per-turn output-token cap (finish_reason=length) and, after an automatic larger-budget retry, still parsed to zero calls — a harness/hardware limit, not a capability failure. If the transcript left no room to retry, the run is context-bound at this hardware.",
      };
    default:
      return { title: "EVALUATION FAILED", detail: "The run did not reach the expected end state on every iteration." };
  }
};

/// Verdict-header color. `reported_in_prose` is the mildest failure (content correct,
/// wrong channel) so it renders TEAL — distinct from the red of a genuine failure — to
/// carry that a wrong-channel model is meaningfully more capable than one that fails hard.
/// `truncated` is a harness/hardware limit (not the model's fault) so it renders AMBER,
/// signalling "a setting/hardware cause" rather than a red capability failure.
export const verdictColor = (topError: string): string =>
  topError === "reported_in_prose" ? "#0f766e" : topError === "truncated" ? "#b45309" : "#991b1b";

const getStepDescription = (kind: string, raw_output: string) => {
  if (kind === "schema_error") {
    return `[Schema validation failed]: ${raw_output}`;
  }
  return raw_output;
};

const getStepNodeStyle = (kind: string, isError: boolean): React.CSSProperties => {
  let bg = "#f0fdf4";
  let border = "1px solid #bbf7d0";
  let color = "#166534";
  
  if (kind === "reported_in_prose") {
    bg = "#f0fdfa";
    border = "1px solid #ccfbf1";
    color = "#0f766e"; // teal — mildest failure, distinct from amber/red
  } else if (kind === "schema_error" || kind === "hallucinated_completion" || kind === "malformed_json" || kind === "foreign_dialect" || kind === "empty_output") {
    bg = "#fffbeb";
    border = "1px solid #fef3c7";
    color = "#b45309";
  } else if (isError) {
    bg = "#fef2f2";
    border = "1px solid #fee2e2";
    color = "#991b1b";
  } else if (kind === "end_state_reached") {
    bg = "#faf5ff";
    border = "1px solid #f3e8ff";
    color = "#6b21a8";
  } else if (kind === "tool_call") {
    bg = "#eff6ff";
    border = "1px solid #dbeafe";
    color = "#1d4ed8";
  }
  
  return {
    position: "absolute",
    left: "-37px",
    top: "12px",
    width: "24px",
    height: "24px",
    borderRadius: "50%",
    background: bg,
    border: border,
    color: color,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 2,
  };
};

const getCardStyle = (kind: string, isError: boolean): React.CSSProperties => {
  let bg = "#ffffff";
  let border = "1px solid #e2e8f0";
  let borderLeft = "3px solid #10b981";
  
  if (kind === "reported_in_prose") {
    bg = "#f0fdfa";
    border = "1px solid #ccfbf1";
    borderLeft = "3px solid #14b8a6"; // teal — mildest failure, distinct from amber/red
  } else if (kind === "schema_error" || kind === "hallucinated_completion" || kind === "malformed_json" || kind === "foreign_dialect" || kind === "empty_output") {
    bg = "#fffbeb";
    border = "1px solid #fef3c7";
    borderLeft = "3px solid #f59e0b";
  } else if (isError) {
    bg = "#fef2f2";
    border = "1px solid #fee2e2";
    borderLeft = "3px solid #ef4444";
  } else if (kind === "end_state_reached") {
    bg = "#faf5ff";
    border = "1px solid #f3e8ff";
    borderLeft = "3px solid #a855f7";
  } else if (kind === "tool_call") {
    bg = "#ffffff";
    border = "1px solid #e2e8f0";
    borderLeft = "3px solid #3b82f6";
  }
  
  return {
    background: bg,
    border: border,
    borderLeft: borderLeft,
    borderRadius: "12px",
    padding: "14px 16px",
    boxShadow: "0 1px 2px 0 rgba(0, 0, 0, 0.02)",
    display: "flex",
    flexDirection: "column",
    gap: 8,
    transition: "all 0.2s",
  };
};

/// One Pass^k repetition's slice of the trajectory. Every agentic task runs k times;
/// the steps stream into a single flat array, so the trace view must re-split them by
/// `run_index` to render "Run N of K" sections with correct per-run turn numbering.
export interface RunGroup {
  runIndex: number;
  steps: TrajectoryStep[];
}

/// Bucket a flat trajectory (all k runs concatenated) into per-run groups, preserving
/// arrival order both across runs and within each run. A run absent from the stream
/// simply doesn't appear; no run_index is assumed contiguous.
export function groupStepsByRun(steps: TrajectoryStep[]): RunGroup[] {
  const groups: RunGroup[] = [];
  const byIndex = new Map<number, RunGroup>();
  for (const s of steps) {
    let g = byIndex.get(s.run_index);
    if (!g) {
      g = { runIndex: s.run_index, steps: [] };
      byIndex.set(s.run_index, g);
      groups.push(g);
    }
    g.steps.push(s);
  }
  return groups;
}

/// The "RUN n OF N" denominator: the CONFIGURED Pass^k, not the runs streamed so far —
/// otherwise a live batch reads "RUN 3 OF 3" then "RUN 4 OF 4" as each rep lands. A
/// completed report carries the intended total (`requested_runs` when the batch was
/// budget-truncated, else `total_runs`); before the terminal outcome lands we fall back to
/// the run's configured `k`. Never drop below the runs actually present, so a stray rep
/// can't read as "RUN 6 OF 5".
export function expectedRunCount(
  report: Pick<AgenticReport, "total_runs" | "requested_runs"> | undefined,
  k: number | undefined,
  groupCount: number,
): number {
  const intended = report ? report.requested_runs ?? report.total_runs : k ?? groupCount;
  return Math.max(intended, groupCount);
}

/// A run passed iff its terminal (last) step reached the end-state. Any other terminal
/// kind — or a run still streaming — is not a pass.
export function runPassed(group: RunGroup): boolean {
  return group.steps[group.steps.length - 1]?.kind === "end_state_reached";
}

/// Below this prefix-reuse fraction, a NON-first turn re-prefilled the transcript instead of
/// reusing the cached prefix — a cache bust (the slow turn). Healthy non-first turns reuse
/// ~0.95+ (the whole prior transcript was just cached); a bust collapses to ~0, so 0.5 sits
/// safely in the gap and never trips on the normal recompute of a turn's own NEW tokens.
export const CACHE_BUST_BELOW = 0.5;

/// Per-turn llama.cpp prefix-cache readout: green when the prefix was reused, amber when it
/// collapsed (a non-first turn re-prefilled), neutral on the first turn (no prior prefix to
/// reuse). Renders nothing when the backend doesn't report it (Ollama/MLX → `available`
/// false). The single `cacheReuse` gate keeps absence-of-feature absent, not a false "0".
export function CacheBadge({ s }: { s: TrajectoryStep }) {
  // cacheReuse(cached = cache_n, recomputed = prefill_tokens); total = cache_n + prefill_tokens.
  const cr = cacheReuse(s.cache_n, s.prefill_tokens);
  if (!cr.available) return null;
  const base = { fontSize: 10, fontWeight: 700, padding: "2px 6px", borderRadius: 4, marginLeft: "auto" } as const;
  if (s.step_index === 0) {
    return (
      <span data-testid="cache-badge-first" title="First turn — the whole prompt is prefilled (there's no prior transcript to reuse yet)." style={{ ...base, background: "#f8fafc", color: "#475569", border: "1px solid #e2e8f0" }}>
        prefill · {cr.total} tok
      </span>
    );
  }
  if (cr.reuseRatio < CACHE_BUST_BELOW) {
    const ms = s.prefill_ms != null ? ` (+${s.prefill_ms}ms)` : "";
    return (
      <span data-testid="cache-badge-bust" title="The reusable transcript prefix was NOT reused this turn — the prompt was re-prefilled (the prefix cache busted: context shifted/evicted). This is the slow turn." style={{ ...base, background: "#fef2f2", color: "#991b1b", border: "1px solid #fee2e2" }}>
        ⚠ CACHE BUST · {cr.recomputed} re-prefilled{ms}
      </span>
    );
  }
  return (
    <span data-testid="cache-badge-hit" title="The transcript prefix was reused from llama.cpp's prefix cache this turn — only the new tokens were recomputed (prefill ≈ 0)." style={{ ...base, background: "#f0fdf4", color: "#15803d", border: "1px solid #dcfce7" }}>
      PREFIX CACHE · {cr.cached} reused / {cr.recomputed} recomputed
    </span>
  );
}

/// The measured reasoning-token total a run spent thinking — summed across its turns. `0` for a
/// terse model (its turns carry no `reasoning_tokens`), so the header hides the segment rather than
/// show a fabricated N/A. A real measured value (backend generated tokens), never an estimate.
export function runThinkingTokens(steps: TrajectoryStep[]): number {
  return steps.reduce((sum, s) => sum + (s.reasoning_tokens ?? 0), 0);
}

/// D9: the two-bar diagnostic for a truncated / reasoning-overrun turn. Shows how many tokens the
/// model spent reasoning AND how full the context window got, then names WHICH limit fired and the
/// fix — a SETTING (raise the thinking preset) vs HARDWARE (bigger machine). The whole point is to
/// keep a budget problem (memory had room) from ever reading as "out of memory".
export function BudgetDiagnostic({ s }: { s: TrajectoryStep }) {
  const reasoning = s.reasoning_tokens ?? 0;
  const used = s.context_used ?? 0;
  const window = s.context_window ?? 0;
  const ctxPct = window > 0 ? Math.min(100, Math.round((used / window) * 100)) : 0;
  const overrun = s.kind === "reasoning_overrun";
  const bar = (label: string, pct: number, note: string, color: string) => (
    <div style={{ marginTop: 6 }}>
      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, color: "#64748b", fontFamily: "Inter, sans-serif" }}>
        <span>{label}</span>
        <span>{note}</span>
      </div>
      <div style={{ height: 6, background: "#f1f5f9", borderRadius: 3, overflow: "hidden", marginTop: 2 }}>
        <div style={{ width: `${pct}%`, height: "100%", background: color }} />
      </div>
    </div>
  );
  return (
    <div data-testid="budget-diagnostic" style={{ marginTop: 8, padding: "10px 12px", borderRadius: 6, background: "#fafafa", border: "1px solid #e2e8f0" }}>
      <div style={{ fontSize: 11, fontWeight: 800, textTransform: "uppercase", letterSpacing: "0.04em", color: "#64748b", fontFamily: "Inter, sans-serif" }}>
        Why this run stopped
      </div>
      {/* Thinking spend: for an overrun it maxed the budget → show ~full amber; context has room. */}
      {bar("Thinking budget", overrun ? 100 : Math.min(100, ctxPct), `${reasoning.toLocaleString()} reasoning tokens`, overrun ? "#f59e0b" : "#94a3b8")}
      {bar("🖥️ Context window", ctxPct, `${used.toLocaleString()} / ${window.toLocaleString()} (${ctxPct}%)`, ctxPct >= 90 ? "#dc2626" : "#94a3b8")}
      <div style={{ marginTop: 8, fontSize: 12, lineHeight: 1.5, fontFamily: "Inter, sans-serif", color: overrun ? "#92400e" : "#991b1b" }}>
        {overrun ? (
          <>
            <b>Setting, not memory.</b> The model spent its whole thinking budget reasoning ({reasoning.toLocaleString()} tok)
            while the context window was only {ctxPct}% full. <b>Fix:</b> raise the thinking preset (Standard → Deep), or the
            model genuinely over-thinks and can't stop.
          </>
        ) : (
          <>
            <b>Hardware limit.</b> The context window filled to {ctxPct}% ({used.toLocaleString()}/{window.toLocaleString()}) —
            this model needs more memory than this machine has. <b>Fix:</b> a bigger machine.
          </>
        )}
      </div>
    </div>
  );
}

export function TraceDebugger({
  model,
  taskId,
  setTaskId,
  decoys,
  tracePass,
  k,
}: TraceDebuggerProps) {
  const { tasks } = useEvalRegistryStore();
  const outcomeByKey = useBatchStore((s) => s.outcomeByKey);
  const nativeOutcomeByKey = useBatchStore((s) => s.nativeOutcomeByKey);
  const stepsByKey = useBatchStore((s) => s.stepsByKey);
  const nativeStepsByKey = useBatchStore((s) => s.nativeStepsByKey);
  const running = useBatchStore((s) => s.running);

  const [activeTab, setActiveTab] = useState<TabType>("trace");
  const [collapsed, setCollapsed] = useState(false);
  // The per-run Input/Output drill-down: which run (null = the single-turn run) and
  // which view. Reset when the task/model changes so it never points at a stale run.
  const [ioRun, setIoRun] = useState<{ runIndex: number | null; mode: RunIoMode } | null>(null);
  // User-toggled expansion overrides per run_index. Empty = follow the default
  // (expand the first failing run, else the first). Reset when the task/model
  // changes so a freshly selected task starts from its own default.
  const [runOverrides, setRunOverrides] = useState<Record<number, boolean>>({});
  useEffect(() => {
    setRunOverrides({});
    setIoRun(null);
  }, [model, taskId]);

  // Find the selected task definition
  const task = tasks.find((t) => t.id === taskId) ?? null;

  // Auto-focus the first task if taskId is not set and tasks are available
  useEffect(() => {
    if (tasks.length > 0 && !taskId) {
      setTaskId(tasks[0].id);
    }
  }, [tasks, taskId, setTaskId]);

  if (!taskId || !task) {
    return (
      <div
        className="rounded-xl overflow-hidden border border-slate-200"
        style={panelStyle}
        data-testid="trace-debugger"
      >
        <div style={{ color: "#64748b", fontSize: 13, fontFamily: "Inter, sans-serif", padding: 20 }}>
          Select a task in the Simulator above to begin single-task pipeline debugging.
        </div>
      </div>
    );
  }

  const key = cellKey(model, taskId);
  const nativeSteps = nativeStepsByKey[key] || [];
  const hasNative = nativeSteps.length > 0;
  // Everything below renders the SELECTED pass: its steps AND its terminal outcome. Gating on
  // the prompt outcome was the bug — with native-first the prompt pass hasn't run, so a live
  // native trace was hidden behind "No trace recorded".
  const onNative = tracePass === "native" && hasNative;
  const steps = (onNative ? nativeSteps : stepsByKey[key]) || [];
  const outcome = onNative ? nativeOutcomeByKey[key] : outcomeByKey[key];
  // Bring-Your-Own diagnostic: no answer key → the per-run chips + final verdict must read
  // "diagnostic" (schema-valid), never PASS/FAIL / "EVALUATION FAILED".
  const diag = outcome?.kind === "agentic" ? outcome.report.diagnostic : undefined;
  // A generated task re-randomizes its entity ids per Pass^k run, so the static `task.prompt`
  // template (used below as the reconstructed preview's fallback) is only right for the one
  // seed that happens to match. Step 0 of whichever run has streamed carries the REAL prompt.
  const realUserPrompt = steps.find((s) => s.step_index === 0)?.initial_prompt;

  // Split the flat trajectory into per-run sections. Runs execute sequentially, so
  // every group but the last is complete; while `running`, the last group may still
  // be streaming (shown as RUNNING rather than a premature FAIL).
  const groups = groupStepsByRun(steps);
  const totalRuns = expectedRunCount(outcome?.kind === "agentic" ? outcome.report : undefined, k, groups.length);
  const isRunComplete = (gi: number) => !running || gi < groups.length - 1;
  // Default-expanded run: the first completed-and-failed run, else the first run.
  const defaultRunIndex =
    groups.find((g, gi) => isRunComplete(gi) && !runPassed(g))?.runIndex ??
    groups[0]?.runIndex ??
    -1;
  const isRunExpanded = (runIndex: number) =>
    runOverrides[runIndex] ?? runIndex === defaultRunIndex;
  const toggleRun = (runIndex: number) =>
    setRunOverrides((o) => ({ ...o, [runIndex]: !isRunExpanded(runIndex) }));

  // The per-run Input/Output button pair. `runIndex` is the agentic run, or null for the
  // single-turn run. `stopPropagation` keeps the agentic buttons from toggling the run's
  // expand when they sit inside the run header row.
  const ioButtons = (runIndex: number | null) => {
    const suffix = runIndex === null ? "" : `-${runIndex}`;
    return (
      <span style={{ display: "inline-flex", gap: 6 }}>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setIoRun({ runIndex, mode: "input" });
          }}
          style={ioBtnStyle}
          data-testid={`trace-io-input${suffix}`}
          title="Show the prompt this run sent to the model"
        >
          Input
        </button>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setIoRun({ runIndex, mode: "output" });
          }}
          style={ioBtnStyle}
          data-testid={`trace-io-output${suffix}`}
          title="Show the model's raw response for this run"
        >
          Output
        </button>
      </span>
    );
  };

  // Tab labels helper
  const renderTabHeader = (id: TabType, label: string) => {
    const isActive = activeTab === id;
    return (
      <button
        key={id}
        type="button"
        onClick={() => setActiveTab(id)}
        style={{
          ...tabBtnStyle,
          background: isActive ? "#eff6ff" : "#ffffff",
          color: isActive ? "#2563eb" : "#475569",
          borderColor: isActive ? "#bfdbfe" : "#e2e8f0",
          fontWeight: isActive ? 600 : 500,
        }}
        data-testid={`evaluator-tab-${id}`}
      >
        {label}
      </button>
    );
  };

  return (
    <div
      className="rounded-2xl overflow-hidden border border-slate-200 shadow-sm transition-all duration-300"
      // Drop the 300px floor when collapsed so the panel shrinks to its header instead
      // of leaving a tall blank white box.
      style={{ ...panelStyle, minHeight: collapsed ? undefined : panelStyle.minHeight }}
      data-testid="trace-debugger"
    >
      {/* Header — the chevron + title is one big toggle button (clear hit target). */}
      <div style={headerStyle}>
        <button
          type="button"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
          data-testid="evaluator-collapse"
          title={collapsed ? "Expand the Evaluator" : "Collapse"}
          style={collapseToggleStyle}
        >
          <span style={chevronStyle} aria-hidden>{collapsed ? "▸" : "▾"}</span>
          <span className="flex h-2.5 w-2.5 rounded-full bg-blue-500" />
          <span style={{ fontSize: 14, fontWeight: 700, color: "#0f172a", fontFamily: "Inter, sans-serif", letterSpacing: "0.03em" }}>
            3. THE EVALUATOR
          </span>
        </button>
        <span style={{ fontSize: 12, color: "#475569", background: "#f1f5f9", padding: "2px 8px", borderRadius: 6, fontFamily: "'JetBrains Mono', monospace", fontWeight: 600, marginLeft: 8 }}>
          {taskId}
        </span>
        {collapsed && (
          <span data-testid="evaluator-collapsed-summary" style={{ marginLeft: 10, fontSize: 12, color: "#64748b", fontFamily: "Inter, sans-serif" }}>
            · click to expand
          </span>
        )}
        {running && (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6, marginLeft: 10, fontSize: 12, color: "#2563eb", fontFamily: "Inter, sans-serif", fontWeight: 600 }} data-testid="evaluator-running">
            <Spinner /> Running…
          </span>
        )}
        <span style={{ marginLeft: "auto" }}>
          <InfoButton {...TOOL_HELP.evaluator} testId="evaluator" />
        </span>
      </div>

      {collapsed ? null : (
      <>
      {/* Tabs Menu */}
      <div style={tabsContainerStyle}>
        {renderTabHeader("config", "Task Config")}
        {renderTabHeader("prompt", "System Prompt Pkg")}
        {renderTabHeader("trace", "Live Trace / Stream")}
        {renderTabHeader("matcher", "AST Matcher")}
      </div>

      {/* Tab Contents */}
      <div style={{ padding: 20 }}>
        {activeTab === "config" && (
          <ConfigPhase task={task} />
        )}

        {activeTab === "prompt" && (
          <SystemMessagePhase
            systemMessage={
              outcome?.kind === "single"
                ? outcome.trace.system_message
                : agenticSystemPreview(task)
            }
            userPrompt={
              outcome?.kind === "single"
                ? outcome.trace.user_prompt
                : (realUserPrompt ?? task.prompt)
            }
          />
        )}

        {activeTab === "trace" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            {/* Which pass this trace is — read-only. The pass is chosen by clicking a Prompt or
                Native result in the Simulator (no in-Evaluator toggle). */}
            {tracePass === "native" && (
              <div data-testid="trace-pass-label" style={{ alignSelf: "flex-start" }}>
                <span
                  style={{
                    fontSize: 11,
                    fontWeight: 700,
                    padding: "3px 10px",
                    borderRadius: 6,
                    border: "1px solid #c4b5fd",
                    background: "#f5f3ff",
                    color: "#6d28d9",
                  }}
                >
                  Tool-Calling (native) trace
                </span>
              </div>
            )}
            {!outcome && steps.length === 0 ? (
              <div style={{ color: "#64748b", fontSize: 13, fontFamily: "Inter, sans-serif", textAlign: "center", padding: 24 }}>
                No trace recorded for this task yet. Run the batch to simulate.
              </div>
            ) : outcome?.kind === "single" ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
                {/* Single turn trace */}
                <div style={{ ...getCardStyle("tool_call", false) }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8, borderBottom: "1px solid #e2e8f0", paddingBottom: 6 }}>
                    <span className="w-2.5 h-2.5 rounded-full bg-emerald-500" />
                    <span style={turnHeaderTitleStyle}>
                      TURN 1: Model Output (Raw text extracted)
                    </span>
                    <span style={{ marginLeft: "auto" }}>{ioButtons(null)}</span>
                  </div>
                  <pre style={codeBlockStyle}>
                    {outcome.trace.raw_output || "(empty output)"}
                  </pre>
                </div>
                
                {/* Verdict */}
                <div style={verdictStyle(outcome.passed)}>
                  {outcome.passed ? (
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <span style={{ color: "#166534", display: "inline-flex", alignItems: "center" }}>
                        <CheckIcon />
                      </span>
                      <div>
                        <div style={{ fontWeight: 700, fontSize: 14, color: "#166534" }}>VERDICT: AST MATCH SUCCESS</div>
                        <div style={{ fontSize: 12, opacity: 0.9, marginTop: 2, color: "#166534" }}>
                          {traceDiag(task, { id: taskId, category: task.category, verdict: outcome.trace.verdict }).msg}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <span style={{ color: "#991b1b", display: "inline-flex", alignItems: "center" }}>
                        <ErrorIcon />
                      </span>
                      <div>
                        <div style={{ fontWeight: 700, fontSize: 14, color: "#991b1b" }}>VERDICT: AST MATCH FAILED</div>
                        <div style={{ fontSize: 12, opacity: 0.9, marginTop: 2, color: "#991b1b" }}>
                          {traceDiag(task, { id: taskId, category: task.category, verdict: outcome.trace.verdict }).msg}
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            ) : outcome?.kind === "agentic" || (!outcome && steps.length > 0) ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
                {/* Chronological agentic turns (renders live from streaming steps even before
                    the terminal outcome lands — the native-first live-trace fix). */}
                {steps.length === 0 ? (
                  <div style={{ color: "#64748b", fontSize: 13, fontFamily: "Inter, sans-serif", textAlign: "center" }}>
                    No steps recorded for this multi-turn run yet.
                  </div>
                ) : (
                  <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                    {groups.map((group, gi) => {
                      const expanded = isRunExpanded(group.runIndex);
                      const complete = isRunComplete(gi);
                      const status: "pass" | "fail" | "running" = !complete
                        ? "running"
                        : runPassed(group)
                        ? "pass"
                        : "fail";
                      return (
                        <div key={group.runIndex} style={runSectionStyle}>
                          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                            <button
                              type="button"
                              onClick={() => toggleRun(group.runIndex)}
                              style={runHeaderStyle}
                              aria-expanded={expanded}
                            >
                              <span style={runCaretStyle}>{expanded ? "▼" : "▶"}</span>
                              <span style={runTitleStyle}>
                                RUN {gi + 1} OF {totalRuns}
                              </span>
                              {diag ? (
                                <span style={runChipStyle("running")}>{complete ? "DIAGNOSTIC" : "RUNNING"}</span>
                              ) : (
                                <span style={runChipStyle(status)}>{status.toUpperCase()}</span>
                              )}
                              {/* How much this run spent thinking — the measured reasoning-token sum
                                  across its turns (a reasoning model only; hidden when 0/absent, never
                                  a fabricated N/A). A subtle violet pill beside the status chip, not
                                  jammed past the right-aligned turn count. */}
                              {runThinkingTokens(group.steps) > 0 && (
                                <span style={runThinkingStyle} title="Reasoning tokens this run spent thinking (the model's reasoning scratchpad across its turns).">
                                  {runThinkingTokens(group.steps).toLocaleString()} thinking tokens
                                </span>
                              )}
                              <span style={runStepCountStyle}>
                                {group.steps.length} {group.steps.length === 1 ? "turn" : "turns"}
                              </span>
                            </button>
                            {ioButtons(group.runIndex)}
                          </div>

                          {expanded && (
                            <div style={hasEnvReplay(group.steps) ? splitRow : undefined}>
                            <div style={{ ...timelineContainer, marginTop: 12, ...(hasEnvReplay(group.steps) ? splitTimeline : null) }}>
                              {group.steps.map((s, index) => {
                                const isError = isErrorKind(s.kind);

                                const icon = getStepIcon(s.kind, isError);
                                const title = getStepTitle(s.kind, isError);
                                const desc = getStepDescription(s.kind, s.raw_output);

                                return (
                                  <div key={index} style={{ position: "relative", marginBottom: index === group.steps.length - 1 ? 0 : 20 }}>
                                    {/* Timeline node dot */}
                                    <div style={getStepNodeStyle(s.kind, isError)}>
                                      {icon}
                                    </div>

                                    {/* Card Content */}
                                    <div style={getCardStyle(s.kind, isError)} className="hover:border-slate-300 transition-all duration-200">
                                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", borderBottom: "1px solid #e2e8f0", paddingBottom: 6, marginBottom: 8 }}>
                                        <span style={turnHeaderTitleStyle}>
                                          TURN {s.step_index + 1}: {title}
                                        </span>
                                        {s.kind === "tool_call" && (
                                          <span style={{ fontSize: 10, fontWeight: 700, padding: "2px 6px", borderRadius: 4, background: "#eff6ff", color: "#1d4ed8", border: "1px solid #dbeafe" }}>
                                            TOOL EXECUTION
                                          </span>
                                        )}
                                        {s.kind === "schema_error" && (
                                          <span style={{ fontSize: 10, fontWeight: 700, padding: "2px 6px", borderRadius: 4, background: "#fffbeb", color: "#b45309", border: "1px solid #fef3c7" }}>
                                            SCHEMA FAILURE
                                          </span>
                                        )}
                                        {s.kind === "tool_error" && (
                                          <span style={{ fontSize: 10, fontWeight: 700, padding: "2px 6px", borderRadius: 4, background: "#fef2f2", color: "#991b1b", border: "1px solid #fee2e2" }}>
                                            FAULT INTERCEPTED
                                          </span>
                                        )}
                                        {/* llama.cpp-only per-turn prefix-cache readout (reused vs recomputed;
                                            amber on a cache bust). Renders nothing for Ollama/MLX. */}
                                        <CacheBadge s={s} />
                                      </div>

                                      <pre style={codeBlockStyle}>
                                        {desc}
                                      </pre>

                                      {/* D9: on a truncated / reasoning-overrun turn, show BOTH the
                                          reasoning spend and the context-window fill, and name which
                                          limit fired (setting vs hardware) — so the user never
                                          mistakes a budget problem for "out of memory". */}
                                      {s.reasoning_tokens != null && s.context_window != null && s.context_used != null && (
                                        <BudgetDiagnostic s={s} />
                                      )}

                                      {/* Sandbox Intercept (if injection context present) */}
                                      {s.injection && (
                                        <div style={sandboxInterceptCard}>
                                          <div style={sandboxHeader}>
                                            Sandbox Response Injection
                                          </div>
                                          <pre style={sandboxBody}>
                                            {s.injection}
                                          </pre>
                                        </div>
                                      )}
                                    </div>
                                  </div>
                                );
                              })}
                            </div>
                            {hasEnvReplay(group.steps) && (
                              <div style={replayCol} data-testid={`env-replay-${group.runIndex}`}>
                                <EnvironmentReplayPanel steps={group.steps} />
                              </div>
                            )}
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* Final Verdict — prompt pass only, and only once the outcome has LANDED (a
                    still-streaming prompt task has steps but no `outcome.report` yet). The native
                    trace shows its own per-run PASS/FAIL chips instead of this. */}
                {tracePass === "prompt" && outcome?.kind === "agentic" && diag && (
                  <div style={{ padding: "12px 14px", borderRadius: 8, background: "#dbeafe", border: "1px solid #93c5fd", display: "flex", alignItems: "center", gap: 12 }}>
                    <div>
                      <div style={{ fontWeight: 700, fontSize: 14, color: "#1e40af" }}>
                        DIAGNOSTIC · schema-valid {diag.schema_valid}/{diag.total_calls}
                      </div>
                      <div style={{ fontSize: 12, opacity: 0.9, marginTop: 2, color: "#1e40af" }}>
                        No answer key — this measures well-formed tool calls + fault attribution (model{" "}
                        {diag.model_faults} · config {diag.config_faults} · server {diag.server_faults}), not pass/fail.
                        {diag.total_calls === 0 && " The model answered in prose without calling a tool."}
                      </div>
                    </div>
                  </div>
                )}
                {tracePass === "prompt" && outcome?.kind === "agentic" && !diag && (
                  <div
                     style={verdictStyle(
                       isStrictPass(outcome.report)
                     )}
                  >
                    {isStrictPass(outcome.report) ? (
                      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                        <span style={{ color: "#166534", display: "inline-flex", alignItems: "center" }}>
                          <CheckIcon />
                        </span>
                        <div>
                          <div style={{ fontWeight: 700, fontSize: 14, color: "#166534" }}>VERDICT: EVALUATION SUCCESS</div>
                          <div style={{ fontSize: 12, opacity: 0.9, marginTop: 2, color: "#166534" }}>All checkpoints reached and expected end-state criteria fully met.</div>
                        </div>
                      </div>
                    ) : (
                      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                        <span style={{ color: verdictColor(outcome.report.top_error), display: "inline-flex", alignItems: "center" }}>
                          <ErrorIcon />
                        </span>
                        <div>
                          <div style={{ fontWeight: 700, fontSize: 14, color: verdictColor(outcome.report.top_error) }}>VERDICT: {verdictLabel(outcome.report.top_error).title}</div>
                          <div style={{ fontSize: 12, opacity: 0.9, marginTop: 2, color: verdictColor(outcome.report.top_error) }}>
                            {verdictLabel(outcome.report.top_error).detail}
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            ) : (
              <div style={{ color: "#b91c1c", fontSize: 13, fontFamily: "Inter, sans-serif", padding: "12px 14px", background: "#fef2f2", border: "1px solid #fee2e2", borderRadius: 8 }}>
                {outcome.message}
              </div>
            )}
          </div>
        )}

        {activeTab === "matcher" && (
          <div>
            {!outcome ? (
              <div style={{ color: "#64748b", fontSize: 13, fontFamily: "Inter, sans-serif" }}>
                No evaluation report available. Run the batch to evaluate.
              </div>
            ) : outcome.kind === "single" ? (
              <VerifyPhase
                verdict={outcome.trace.verdict}
                category={task.category}
              />
            ) : outcome.kind === "agentic" ? (
              <VerifyPhase
                verdict={{
                  parsed: outcome.report.failures.malformed_json_calls === 0,
                  tool_match: outcome.report.failures.infinite_loop_hits === 0,
                  args_match: isStrictPass(outcome.report),
                  abstain_correct: null,
                }}
                category={task.category}
              />
            ) : (
              <div style={{ color: "#b91c1c", fontSize: 13, fontFamily: "Inter, sans-serif" }}>
                Error loading matcher report.
              </div>
            )}
          </div>
        )}
      </div>
      </>
      )}

      {ioRun && (() => {
        const isSingle = ioRun.runIndex === null;
        const runSteps = isSingle ? [] : steps.filter((s) => s.run_index === ioRun.runIndex);
        let title = "Single-turn run";
        if (!isSingle) {
          const pos = groups.findIndex((g) => g.runIndex === ioRun.runIndex);
          title = pos >= 0 ? `RUN ${pos + 1} OF ${groups.length}` : `RUN ${ioRun.runIndex}`;
        }
        return (
          <RunIoModal
            task={task}
            outcome={outcome}
            steps={runSteps}
            title={title}
            decoys={decoys}
            mode={ioRun.mode}
            setMode={(mode) => setIoRun({ runIndex: ioRun.runIndex, mode })}
            onClose={() => setIoRun(null)}
          />
        );
      })()}
    </div>
  );
}

// ── Styles ──────────────────────────────────────────────────────────────────

const ioBtnStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
  color: "#2563eb",
  background: "#eff6ff",
  border: "1px solid #dbeafe",
  borderRadius: 6,
  padding: "2px 10px",
  cursor: "pointer",
};

const panelStyle: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #e2e8f0",
  borderRadius: "16px",
  boxShadow: "0 1px 3px 0 rgba(0, 0, 0, 0.05)",
  minHeight: 300,
};

const headerStyle: React.CSSProperties = {
  padding: "16px 20px",
  borderBottom: "1px solid #e2e8f0",
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  background: "#fafafa",
};

const collapseToggleStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 8,
  background: "transparent",
  border: "none",
  cursor: "pointer",
  padding: 0,
  textAlign: "left",
};

/// The visible disclosure chevron — a 22px rounded chip so it reads as a control,
/// not a stray glyph (mirrors the Simulator header).
const chevronStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 22,
  height: 22,
  borderRadius: 6,
  background: "#e2e8f0",
  color: "#334155",
  fontSize: 12,
  fontWeight: 800,
  lineHeight: 1,
  flexShrink: 0,
};

const tabsContainerStyle: React.CSSProperties = {
  display: "flex",
  gap: 8,
  padding: "12px 20px 6px",
  borderBottom: "1px solid #e2e8f0",
  flexWrap: "wrap",
  background: "#ffffff",
};

const tabBtnStyle: React.CSSProperties = {
  padding: "6px 14px",
  borderRadius: 8,
  fontSize: 12,
  fontFamily: "Inter, sans-serif",
  cursor: "pointer",
  borderWidth: 1,
  borderStyle: "solid",
  transition: "all 0.15s ease",
};

const codeBlockStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 13,
  lineHeight: 1.5,
  color: "#334155",
  fontFamily: "'JetBrains Mono', monospace",
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
};

const verdictStyle = (passed: boolean): React.CSSProperties => {
  return {
    background: passed ? "#f0fdf4" : "#fef2f2",
    border: `1px solid ${passed ? "#bbf7d0" : "#fee2e2"}`,
    borderRadius: 12,
    padding: "14px 18px",
    fontSize: 13,
    fontWeight: 600,
    color: passed ? "#166534" : "#991b1b",
    fontFamily: "Inter, sans-serif",
    marginTop: 8,
  };
};

const timelineContainer: React.CSSProperties = {
  borderLeft: "2px dashed #cbd5e1",
  marginLeft: "18px",
  paddingLeft: "24px",
  display: "flex",
  flexDirection: "column",
  gap: "18px",
  position: "relative",
};

// Split-view: text trace (left) + visual environment replay (right), for environment tasks.
const splitRow: React.CSSProperties = { display: "flex", gap: 12, alignItems: "flex-start", marginTop: 12 };
const splitTimeline: React.CSSProperties = { flex: "1 1 0", minWidth: 0, marginTop: 0 };
const replayCol: React.CSSProperties = { flex: "1 1 0", minWidth: 0, position: "sticky", top: 12 };

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
  width: "100%",
  background: "transparent",
  border: "none",
  padding: 0,
  cursor: "pointer",
  textAlign: "left",
};

const runCaretStyle: React.CSSProperties = {
  fontSize: 10,
  color: "#64748b",
  width: 12,
  flexShrink: 0,
};

const runTitleStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 800,
  color: "#475569",
  fontFamily: "Inter, sans-serif",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const runStepCountStyle: React.CSSProperties = {
  marginLeft: "auto",
  fontSize: 11,
  color: "#94a3b8",
  fontFamily: "Inter, sans-serif",
};

/// The per-run reasoning-token badge in the RUN header — a subtle violet PILL beside the
/// PASS/FAIL chip so it reads as the model's "thinking" effort, distinct from the neutral
/// turn count (which is right-aligned; the badge no longer hides past it).
const runThinkingStyle: React.CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: "#7c3aed",
  fontFamily: "Inter, sans-serif",
  background: "#f5f3ff",
  border: "1px solid #ddd6fe",
  borderRadius: 4,
  padding: "2px 8px",
};

/// PASS (green) / FAIL (red) / RUNNING (blue) chip for a Pass^k run header.
const runChipStyle = (status: "pass" | "fail" | "running"): React.CSSProperties => {
  const palette = {
    pass: { bg: "#f0fdf4", color: "#166534", border: "#bbf7d0" },
    fail: { bg: "#fef2f2", color: "#991b1b", border: "#fee2e2" },
    running: { bg: "#eff6ff", color: "#1d4ed8", border: "#dbeafe" },
  }[status];
  return {
    fontSize: 10,
    fontWeight: 700,
    padding: "2px 8px",
    borderRadius: 4,
    background: palette.bg,
    color: palette.color,
    border: `1px solid ${palette.border}`,
    fontFamily: "Inter, sans-serif",
    letterSpacing: "0.03em",
  };
};

const sandboxInterceptCard: React.CSSProperties = {
  marginTop: 10,
  background: "#f8fafc",
  border: "1px solid #e2e8f0",
  borderRadius: 8,
  padding: "10px 12px",
};

const sandboxHeader: React.CSSProperties = {
  fontSize: 10,
  fontWeight: 800,
  textTransform: "uppercase",
  color: "#64748b",
  fontFamily: "Inter, sans-serif",
  letterSpacing: "0.04em",
  marginBottom: 6,
};

const sandboxBody: React.CSSProperties = {
  margin: 0,
  fontSize: 12,
  fontFamily: "'JetBrains Mono', monospace",
  color: "#2563eb",
  whiteSpace: "pre-wrap",
};

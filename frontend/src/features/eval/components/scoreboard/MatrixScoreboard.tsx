import { useState } from "react";
import { useEvalRegistryStore } from "../../state/evalRegistryStore";
import { useInstalledModelsStore } from "../../../models/state/installedModelsStore";
import { useBatchStore, cellKey } from "../../state/batchStore";
import { modelLabel } from "../../../../shared/models/modelLabel";
import { isStrictPass, dialectLabel, type TaskOutcome } from "../../../../shared/ipc/eval/batch";
import type { ToolTask } from "../../../../shared/ipc/eval/registry";
import { InfoButton } from "../../../../shared/ui/InfoButton";
import { Spinner } from "../../../../shared/ui/Spinner";
import { RunProgress } from "./RunProgress";
import { TOOL_HELP, metricTitle } from "../../help";

interface MatrixScoreboardProps {
  model: string;
  k: number;
  maxSteps: number;
  /// Phase 9 run shape, echoed as header chips: the difficulty tier label and the
  /// decoy-tool budget (`undefined` → "off"). They describe the LAST/active run's
  /// levers alongside the target model.
  tierLabel?: string;
  decoys?: number;
  focusedTaskId: string | null;
  setFocusedTaskId: (taskId: string | null) => void;
  /// The pass the Evaluator is showing — set when a Prompt vs Native result is clicked.
  focusedPass: "prompt" | "native";
  setFocusedPass: (p: "prompt" | "native") => void;
}

export function MatrixScoreboard({
  model,
  k,
  maxSteps,
  tierLabel,
  decoys,
  focusedTaskId,
  setFocusedTaskId,
  focusedPass,
  setFocusedPass,
}: MatrixScoreboardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const { tasks } = useEvalRegistryStore();
  const list = useInstalledModelsStore((s) => s.list);
  const running = useBatchStore((s) => s.running);
  const progress = useBatchStore((s) => s.progress);
  const live = useBatchStore((s) => s.live);
  const outcomeByKey = useBatchStore((s) => s.outcomeByKey);
  const nativeOutcomeByKey = useBatchStore((s) => s.nativeOutcomeByKey);
  const stepsByKey = useBatchStore((s) => s.stepsByKey);
  const nativeStepsByKey = useBatchStore((s) => s.nativeStepsByKey);
  const error = useBatchStore((s) => s.error);

  // Which passes this run measured — from outcomes OR live steps, so the column appears the
  // instant a pass streams (not only after a task finishes). A native-only run shows only the
  // Tool-Calling columns (no empty Prompt-based / PB Steps); prompt-only shows only those.
  const hasNative = tasks.some((t) => {
    const key = cellKey(model, t.id);
    return nativeOutcomeByKey[key] != null || (nativeStepsByKey[key]?.length ?? 0) > 0;
  });
  const hasPromptData = tasks.some((t) => {
    const key = cellKey(model, t.id);
    return outcomeByKey[key] != null || (stepsByKey[key]?.length ?? 0) > 0;
  });
  // Prompt columns show when prompt ran — or as the default when native hasn't appeared yet
  // (so a fresh run isn't a column-less table).
  const showPrompt = hasPromptData || !hasNative;

  // Open the Evaluator on a specific (task, pass) — used by both result cells so clicking a
  // Native pass/fail opens the native trace and a Prompt one opens the prompt trace.
  const openTrace = (taskId: string, pass: "prompt" | "native") => {
    setFocusedTaskId(taskId);
    setFocusedPass(pass);
  };

  const mInfo = list.find((x) => x.name === model);
  const modelTargetLabel = mInfo
    ? `${modelLabel(mInfo)}${mInfo.quantization ? ` (${mInfo.quantization})` : ""}`
    : model || "None";

  // Aggregate stats for ONE pass's outcomes (native OR prompt), computed per-pass and
  // never blended: native FC and the prompt-based proxy are measured differently, so a
  // single combined average would be uninterpretable (eval-metric comparability).
  const aggregate = (byKey: Record<string, TaskOutcome>) => {
    let totalPasses = 0;
    let totalRuns = 0;
    let totalSteps = 0;
    let stepsCount = 0;
    let totalTokens = 0;
    let tokensCount = 0;
    for (const t of tasks) {
      const outcome = byKey[cellKey(model, t.id)];
      if (!outcome) continue;
      if (outcome.kind === "single") {
        totalPasses += outcome.passed ? 1 : 0;
        totalRuns += 1;
        totalSteps += 1;
        stepsCount += 1;
        if (outcome.trace.prompt_tokens != null) {
          totalTokens += outcome.trace.prompt_tokens;
          tokensCount += 1;
        }
      } else if (outcome.kind === "agentic" && outcome.report.diagnostic) {
        // Bring-Your-Own diagnostic: a schema-valid rate, not a pass^k — keep it OUT of the
        // pass-rate aggregate so it's never averaged with (or read as) task completions.
      } else if (outcome.kind === "agentic") {
        totalPasses += outcome.report.passes;
        totalRuns += outcome.report.total_runs;
        if (outcome.report.avg_steps != null) {
          totalSteps += outcome.report.avg_steps * outcome.report.total_runs;
          stepsCount += outcome.report.total_runs;
        }
        if (outcome.report.avg_output_tokens_success != null) {
          totalTokens += outcome.report.avg_output_tokens_success * outcome.report.passes;
          tokensCount += outcome.report.passes;
        }
      }
    }
    return {
      totalPasses,
      totalRuns,
      passRate: totalRuns > 0 ? Math.round((totalPasses / totalRuns) * 100) : 0,
      avgStepsVal: stepsCount > 0 ? (totalSteps / stepsCount).toFixed(1) : "—",
      effortVal: tokensCount > 0 ? Math.round(totalTokens / tokensCount).toLocaleString() : "—",
    };
  };

  // One aggregate per pass — the Tool-Calling (native) AGGREGATE was previously blind to
  // native outcomes (the loop read only `outcomeByKey`), so a native-only run showed "—".
  const nativeAgg = aggregate(nativeOutcomeByKey);
  const promptAgg = aggregate(outcomeByKey);
  const anyRuns = nativeAgg.totalRuns > 0 || promptAgg.totalRuns > 0;
  // The collapsed-summary chip reads whichever pass ran (native first, else prompt).
  const summaryAgg = nativeAgg.totalRuns > 0 ? nativeAgg : promptAgg;

  return (
    <div
      className="rounded-xl overflow-hidden border border-slate-200 shadow-sm"
      style={panelStyle}
      data-testid="matrix-scoreboard"
    >
      {/* Header — the chevron + title is one big toggle button (clear hit target). */}
      <div style={headerStyle}>
        <button
          type="button"
          onClick={() => setCollapsed((c) => !c)}
          aria-expanded={!collapsed}
          data-testid="simulator-collapse"
          title={collapsed ? "Expand the Simulator" : "Collapse"}
          style={collapseToggleStyle}
        >
          <span style={chevronStyle} aria-hidden>{collapsed ? "▸" : "▾"}</span>
          <span style={{ fontSize: 15, fontWeight: 700, color: "#0f172a", fontFamily: "Inter, sans-serif" }}>
            2. THE SIMULATOR (Batch Scoreboard)
          </span>
        </button>
        <span
          style={{ fontSize: 13, color: "#2563eb", fontFamily: "'JetBrains Mono', monospace", fontWeight: 650 }}
          data-testid="scoreboard-run-chips"
        >
          &nbsp;- [ Target: {modelTargetLabel}
          {tierLabel ? ` · Tier: ${tierLabel}` : ""} · K: {k} · Decoys: {decoys ?? "off"} ]
        </span>
        {collapsed && (
          <span data-testid="simulator-collapsed-summary" style={{ marginLeft: 10, fontSize: 12, color: "#64748b", fontFamily: "Inter, sans-serif" }}>
            · {summaryAgg.totalRuns > 0 ? `${summaryAgg.passRate}% pass` : "no run yet"} · click to expand
          </span>
        )}
        {running && (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6, marginLeft: 10, fontSize: 12, color: "#2563eb", fontFamily: "Inter, sans-serif", fontWeight: 600 }} data-testid="simulator-running">
            <Spinner /> Running…
          </span>
        )}
        <span style={{ marginLeft: "auto" }}>
          <InfoButton {...TOOL_HELP.simulator} testId="simulator" />
        </span>
      </div>

      {collapsed ? null : (
      <>
      {error && (
        <div style={{ padding: "10px 16px", color: "#dc2626", fontSize: 12, fontFamily: "Inter, sans-serif" }} data-testid="scoreboard-error">
          {error}
        </div>
      )}

      {/* Live progress (visible during a run): task position, current Pass^k run +
          turn, the model's last action, and an elapsed clock — so even a stalled
          looping run visibly advances instead of looking hung. */}
      {running && <RunProgress done={progress.done} total={progress.total} live={live} k={k} maxSteps={maxSteps} />}

      {/* Inner Box Content */}
      <div style={{ padding: 16 }}>
        {/* Aggregate Stats — one labeled line per measured pass (native + prompt are kept
            separate, never averaged together). A single-pass run shows a single line; the
            "AGGREGATE" heading stays so a no-run state still reads as the aggregate box. */}
        <div style={aggregateBoxStyle}>
          <div style={{ fontWeight: 800, color: "#0f172a", marginBottom: anyRuns ? 5 : 0, letterSpacing: "0.04em" }}>
            AGGREGATE
          </div>
          {anyRuns ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {nativeAgg.totalRuns > 0 && (
                <span data-testid="aggregate-native">
                  <strong style={{ color: "#0f172a" }}>Tool-Calling (native):</strong>{" "}
                  <strong style={{ color: "#166534" }} title={metricTitle("passRate")}>{nativeAgg.passRate}% Pass Rate</strong> ({nativeAgg.totalPasses}/{nativeAgg.totalRuns}) | <span title={metricTitle("avgSteps")}>Avg Steps:</span> <strong>{nativeAgg.avgStepsVal}</strong> | <span title={metricTitle("effort")}>Effort:</span> <strong>{nativeAgg.effortVal}</strong> tokens
                </span>
              )}
              {promptAgg.totalRuns > 0 && (
                <span data-testid="aggregate-prompt">
                  <strong style={{ color: "#0f172a" }}>Prompt-based:</strong>{" "}
                  <strong style={{ color: "#166534" }} title={metricTitle("passRate")}>{promptAgg.passRate}% Pass Rate</strong> ({promptAgg.totalPasses}/{promptAgg.totalRuns}) | <span title={metricTitle("avgSteps")}>Avg Steps:</span> <strong>{promptAgg.avgStepsVal}</strong> | <span title={metricTitle("effort")}>Effort:</span> <strong>{promptAgg.effortVal}</strong> tokens
                </span>
              )}
            </div>
          ) : (
            <span style={{ color: "#64748b" }}>— Pass Rate | Avg Steps: — | Effort: —</span>
          )}
        </div>

        {/* Tasks Table */}
        {tasks.length === 0 ? (
          <div style={{ padding: "20px 0", color: "#64748b", fontSize: 13, fontFamily: "Inter, sans-serif", textAlign: "center" }}>
            No tasks in this collection.
          </div>
        ) : (
          <div style={{ overflowX: "auto" }}>
            <table style={{ width: "100%", borderCollapse: "collapse", minWidth: 600 }} data-testid="scoreboard-table">
              <thead>
                <tr style={{ borderBottom: "1px solid #e2e8f0" }}>
                  <th style={thStyle}>Task ID</th>
                  <th style={thStyle}>Category</th>
                  <th style={thStyle}>Target Tool</th>
                  {hasNative && <th style={thStyle}>Tool-Calling</th>}
                  {hasNative && <th style={thStyle} title="Avg steps of the Tool-Calling (native) run">TC Steps</th>}
                  {showPrompt && <th style={thStyle}>Prompt-based</th>}
                  {showPrompt && <th style={thStyle} title="Avg steps of the Prompt-based run">PB Steps</th>}
                </tr>
              </thead>
              <tbody>
                {tasks.map((t) => {
                  const key = cellKey(model, t.id);
                  const outcome = outcomeByKey[key];
                  
                  // Map category to display text
                  let categoryLabel: string = t.category;
                  if (t.category === "single") categoryLabel = "Single-Turn";
                  else if (t.category === "agentic") categoryLabel = "Multi-Step";
                  else if (t.category === "parallel") categoryLabel = "Parallel";
                  else if (t.category === "select") categoryLabel = "Select";
                  else if (t.category === "abstain") categoryLabel = "Abstain";

                  // Per-task PROMPT and NATIVE outcomes — each rendered in its own clickable
                  // result + steps columns so the user sees (and opens) each pass independently.
                  const nativeOutcome = nativeOutcomeByKey[key];
                  const isActive = focusedTaskId === t.id;
                  const targetTool = targetToolFor(t);

                  return (
                    <tr
                      key={t.id}
                      onClick={() => openTrace(t.id, showPrompt ? "prompt" : "native")}
                      style={{
                        ...trStyle,
                        cursor: "pointer",
                        background: isActive ? "#eff6ff" : "transparent",
                      }}
                      data-testid={`scoreboard-row-${t.id}`}
                      title="Click a Prompt or Native result to open that trace in the Evaluator below"
                    >
                      <td style={{ ...tdStyle, fontFamily: "'JetBrains Mono', monospace", color: isActive ? "#2563eb" : "#0f172a" }}>
                        {t.id}
                      </td>
                      <td style={tdStyle}>{categoryLabel}</td>
                      <td style={{ ...tdStyle, fontFamily: "'JetBrains Mono', monospace", color: "#64748b", fontSize: 12 }}>
                        {targetTool}
                      </td>
                      {/* TOOL-CALLING (native) result + its OWN steps — only once measured; runs
                          first, so it sits before Prompt-based. Clicking opens the native trace. */}
                      {hasNative && (
                        <>
                          <td
                            style={{ ...tdStyle, cursor: "pointer" }}
                            onClick={(e) => { e.stopPropagation(); openTrace(t.id, "native"); }}
                            data-testid={`native-cell-${t.id}`}
                            title="Open the Tool-Calling (native) trace in the Evaluator"
                          >
                            <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                              {resultBadge(nativeOutcome, `result-native-${t.id}`)}
                              {isActive && focusedPass === "native" && <span style={{ color: "#2563eb", fontSize: 14 }}>◄</span>}
                            </div>
                          </td>
                          <td
                            style={{ ...tdStyle, cursor: "pointer" }}
                            onClick={(e) => { e.stopPropagation(); openTrace(t.id, "native"); }}
                            data-testid={`native-steps-${t.id}`}
                          >
                            {stepsOf(nativeOutcome)}
                          </td>
                        </>
                      )}
                      {/* PROMPT-BASED result + its OWN steps — only when the prompt pass ran;
                          clicking opens the prompt trace. */}
                      {showPrompt && (
                        <>
                          <td
                            style={{ ...tdStyle, cursor: "pointer" }}
                            onClick={(e) => { e.stopPropagation(); openTrace(t.id, "prompt"); }}
                            data-testid={`prompt-cell-${t.id}`}
                            title="Open the prompt-based trace in the Evaluator"
                          >
                            <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                              {resultBadge(outcome, `result-${t.id}`)}
                              {isActive && focusedPass === "prompt" && <span style={{ color: "#2563eb", fontSize: 14 }}>◄</span>}
                            </div>
                          </td>
                          <td
                            style={{ ...tdStyle, cursor: "pointer" }}
                            onClick={(e) => { e.stopPropagation(); openTrace(t.id, "prompt"); }}
                            data-testid={`prompt-steps-${t.id}`}
                          >
                            {stepsOf(outcome)}
                          </td>
                        </>
                      )}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <div style={{ fontSize: 11, color: "#475569", fontFamily: "Inter, sans-serif", marginTop: 10 }}>
          <strong>TC Steps</strong> / <strong>PB Steps</strong> = avg steps of the Tool-Calling (native)
          vs Prompt-based run (single-turn = 1; Multi-Step = avg across the K runs). Target Tool is the
          task's expected call, or — for a Multi-Step task — the tools its end-state requires.
        </div>
      </div>
      </>
      )}
    </div>
  );
}

/// The task's "target tool": the expected call for single/parallel tasks, or — for an agentic
/// task (whose `expected` is `no_call`) — the tools its end-state actually requires, so the
/// column isn't a blank "—" for every multi-step task.
function targetToolFor(t: ToolTask): string {
  if (t.expected.type === "call") return t.expected.name;
  if (t.expected.type === "parallel") return t.expected.calls.map((c) => c.name).join(", ");
  const es = t.agentic?.end_state;
  // A state-diff (web-UI) task is graded on the final state, not tool checkpoints, so there's no
  // checkpoint tool to name. Show the action tools the model drives the UI with: the declared
  // tools minus the must-not-call decoys (so the trap isn't listed as a target).
  if (es && typeof es === "object" && "require_end_state" in es) {
    const forbidden = new Set((t.agentic?.must_not_call ?? []).map(mustNotCallName).filter((n): n is string => n != null));
    const actions = t.tools.map((tool) => tool.name).filter((n) => !forbidden.has(n));
    return actions.length ? actions.join(", ") : "—";
  }
  const cps = es && typeof es === "object" ? ("require_all" in es ? es.require_all : "require_sequence" in es ? es.require_sequence : []) : [];
  const names = Array.from(new Set(cps.map((c) => c.tool)));
  return names.length ? names.join(", ") : "—";
}

/// A `must_not_call` entry is a bare tool name or a `{ name, args }` pair (opaque `unknown` to the
/// frontend) — pull the tool name out of either form.
function mustNotCallName(m: unknown): string | null {
  if (typeof m === "string") return m;
  if (m && typeof m === "object" && "name" in m && typeof (m as { name: unknown }).name === "string") {
    return (m as { name: string }).name;
  }
  return null;
}

/// Avg steps for ONE pass's outcome (single = 1 turn; agentic = avg over the K runs). `—` until
/// that pass produces this task's outcome — so Tool-Calling and Prompt-based each show their own.
function stepsOf(outcome: TaskOutcome | undefined): string {
  if (!outcome) return "—";
  if (outcome.kind === "single") return "1";
  if (outcome.kind === "agentic") return outcome.report.avg_steps != null ? (Math.round(outcome.report.avg_steps * 10) / 10).toString() : "—";
  return "—";
}

/// The pass/fail badge (+ dialect chip) for one task outcome — shared by the Prompt and Native
/// result columns so both render identically. `—` until that pass produces this task's outcome.
/// A PARTIAL Pass^k (e.g. 3/5) is "Unreliable" (amber), never a flat Fail; a budget-truncated
/// run is never a clean Pass (`isStrictPass`) — the un-run reps were never observed.
function resultBadge(outcome: TaskOutcome | undefined, testId: string): React.ReactNode {
  if (!outcome) return <span style={{ color: "#cbd5e1" }}>—</span>;
  // A non-standard tool-call dialect (e.g. Harmony) the calls were normalized from — flagged so
  // a model that only scored via its native grammar is visible, not silently credited.
  let dialectEl: React.ReactNode = null;
  if (outcome.kind === "agentic") {
    const dl = dialectLabel(outcome.report.dialect);
    if (dl) {
      dialectEl = (
        <span
          style={dialectChipStyle}
          title={`Model emitted the ${dl} tool-call dialect instead of the instructed JSON — its calls were normalized so the run could be scored.`}
        >
          {dl}
        </span>
      );
    }
  }
  let badge: React.ReactNode = <span style={{ color: "#64748b" }}>—</span>;
  if (outcome.kind === "single") {
    badge = outcome.passed ? <span style={passBadgeStyle}>Pass</span> : <span style={failBadgeStyle}>Fail</span>;
  } else if (outcome.kind === "agentic" && outcome.report.diagnostic) {
    // Bring-Your-Own: no answer key → a well-formedness rate, NOT a pass^k verdict.
    const d = outcome.report.diagnostic;
    const pct = d.total_calls ? Math.round((d.schema_valid / d.total_calls) * 100) : 0;
    badge = (
      <span
        style={diagnosticBadgeStyle}
        data-testid={testId}
        title={`Diagnostic — no answer key. ${d.schema_valid}/${d.total_calls} calls schema-valid; faults → model ${d.model_faults}, config ${d.config_faults}, server ${d.server_faults}.`}
      >
        schema-valid {d.schema_valid}/{d.total_calls} ({pct}%)
      </span>
    );
  } else if (outcome.kind === "agentic") {
    const { passes, total_runs, requested_runs } = outcome.report;
    const truncated = requested_runs != null;
    badge = isStrictPass(outcome.report) ? (
      <span style={passBadgeStyle} data-testid={testId}>Pass</span>
    ) : passes === 0 ? (
      <span style={failBadgeStyle} data-testid={testId}>Fail</span>
    ) : (
      <span
        style={partialBadgeStyle}
        data-testid={testId}
        title={
          truncated
            ? `${passes}/${total_runs} of ${requested_runs} runs passed — stopped at the time budget, incomplete, not a clean pass`
            : `${passes}/${total_runs} runs passed — unreliable, not a clean pass`
        }
      >
        Partial {passes}/{total_runs}{truncated ? ` of ${requested_runs}` : ""}
      </span>
    );
  } else if (outcome.kind === "error") {
    badge = <span style={failBadgeStyle}>Error</span>;
  }
  return (
    <div style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      {badge}
      {dialectEl}
    </div>
  );
}

// ── Styles ──────────────────────────────────────────────────────────────────

const panelStyle: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #e2e8f0",
  borderRadius: "16px",
  boxShadow: "0 1px 3px 0 rgba(0, 0, 0, 0.05)",
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
  gap: 10,
  background: "transparent",
  border: "none",
  cursor: "pointer",
  padding: 0,
  textAlign: "left",
};

/// The visible disclosure chevron — a 22px rounded chip so it reads as a control,
/// not a stray glyph. Shared shape across the Simulator/Evaluator headers.
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

const aggregateBoxStyle: React.CSSProperties = {
  background: "#f8fafc",
  border: "1px solid #e2e8f0",
  borderRadius: 8,
  padding: "10px 16px",
  fontSize: 13,
  color: "#334155",
  fontFamily: "Inter, sans-serif",
  marginBottom: 16,
  letterSpacing: "-0.01em",
};

const thStyle: React.CSSProperties = {
  textAlign: "left",
  fontSize: 10,
  fontWeight: 800,
  color: "#64748b",
  textTransform: "uppercase",
  letterSpacing: "0.06em",
  padding: "10px 12px",
  fontFamily: "Inter, sans-serif",
};

const tdStyle: React.CSSProperties = {
  fontSize: 13,
  color: "#334155",
  padding: "10px 12px",
  fontFamily: "Inter, sans-serif",
};

const trStyle: React.CSSProperties = {
  borderBottom: "1px solid #e2e8f0",
  transition: "background 0.15s ease",
};

const passBadgeStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "#dcfce7",
  border: "1px solid #bbf7d0",
  color: "#166534",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
};

const partialBadgeStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "#fef3c7",
  border: "1px solid #fcd34d",
  color: "#92400e",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
};

// Non-standard tool-call dialect chip (e.g. "Harmony") — violet, distinct from the
// pass/fail/partial verdict colors so it reads as metadata, not a result.
const dialectChipStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  background: "#f5f3ff",
  border: "1px solid #ddd6fe",
  color: "#6d28d9",
  borderRadius: 6,
  padding: "1px 6px",
  fontSize: 11,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
};

/// Bring-Your-Own diagnostic score — deliberately NOT green/amber/red (which read as a
/// pass/fail verdict). A neutral blue "schema-valid X/Y" says "measured, not judged".
const diagnosticBadgeStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "#dbeafe",
  border: "1px solid #93c5fd",
  color: "#1e40af",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
};

const failBadgeStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 5,
  background: "#fee2e2",
  border: "1px solid #fca5a5",
  color: "#991b1b",
  borderRadius: 6,
  padding: "2px 8px",
  fontSize: 12,
  fontWeight: 600,
  fontFamily: "Inter, sans-serif",
};


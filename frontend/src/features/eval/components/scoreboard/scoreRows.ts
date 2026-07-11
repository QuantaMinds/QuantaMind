import type { BatchReport, FailureTracker, TopError } from "../../../../shared/ipc/eval/batch";
import type { InstalledModelInfo } from "../../../../shared/ipc/models/storage";
import { modelLabel } from "../../../../shared/models/modelLabel";

const TOP_ERROR_LABEL: Record<TopError, string> = {
  none: "None",
  infinite_loop: "Loop Cap",
  hallucinated: "Fake Done",
  malformed_json: "Malformed",
  malformed_schema: "Bad Schema",
  forbidden_call: "Forbidden",
  turn_timeout: "Timeout",
  reported_in_prose: "Wrong Channel",
  foreign_dialect: "Bad Dialect",
  empty_output: "No Output",
  truncated: "Truncated (HW)",
  reasoning_overrun: "Over-reasoned",
};

/// Why an all-errored native pass failed (every run threw a backend Err → no scored result).
/// SchemaRejected = the model/template can't express tool calls natively (the honest "this
/// model can't do native FC"); InfraHost = the backend crashed/timed out (blame the machine).
const NATIVE_ERROR_LABEL: Record<"none" | "infra_host" | "schema_rejected" | "mixed", string> = {
  none: "Native failed",
  schema_rejected: "Native unsupported",
  infra_host: "Backend error",
  mixed: "Native failed",
};

/// One per-model row of the Matrix Scoreboard. Every metric is a display string;
/// null/inapplicable sources render "N/A" (agentic metrics on a column that had
/// none) or "—" (single-turn rows have no steps/effort) — never a fabricated 0.
export interface ScoreRow {
  model: string;
  label: string;
  quant: string;
  passK: string;
  /// Phase 7.2 native function-calling Pass^k (Ollama `/api/chat` tool_calls),
  /// "N/A" when native wasn't measured for this model. Shown behind a toggle.
  passKNative: string;
  avgSteps: string;
  /// Avg steps of the NATIVE (Tool-Calling) run, "N/A" when native wasn't measured — shown next
  /// to the native Pass^k so the two passes' step costs are comparable, not conflated.
  avgStepsNative: string;
  effort: string;
  /// T* — total generated tokens ÷ completed tasks (amortized cost incl. failed-run waste).
  /// "—" when nothing completed. Distinct from `effort` (successes-only average).
  tokensPerTask: string;
  schemaResil: string;
  topError: string;
  /// `true` when the NATIVE (Tool-Calling) pass was measured for this model — the matrix renders
  /// a Tool-Calling row only then.
  hasNative: boolean;
  /// `true` when the PROMPT-based pass was measured (agentic/toolcall/error present) — the matrix
  /// renders a Prompt-based row only then, so a native-only run shows no empty prompt row.
  hasPrompt: boolean;
  /// Native-pass counterparts, used for the Tool-Calling row. `null`/N-A when native wasn't run.
  effortNative: string;
  tokensPerTaskNative: string;
  schemaResilNative: string;
  topErrorNative: string;
  failuresNative: FailureTracker | null;
  /// The full agentic failure breakdown (all 4 counts) behind `topError`, so the UI
  /// can surface the two it hides (Fake Done / Bad Schema). `null` for single-turn
  /// or errored columns (no agentic run).
  failures: FailureTracker | null;
  composite: string;
}

const fmtNum = (n: number | null) => (n == null ? "N/A" : (Math.round(n * 10) / 10).toString());
const fmtTokens = (n: number | null) => (n == null ? "N/A" : `${Math.round(n)} tok`);
const fmtPct = (n: number | null | undefined) => (n == null ? "—" : `${Math.round(n * 100)}%`);

export function toScoreRows(report: BatchReport | null, models: InstalledModelInfo[]): ScoreRow[] {
  if (!report) return [];
  return report.columns.map((c) => {
    const info = models.find((m) => m.name === c.model);
    const ag = c.agentic;
    // The Pass column is unified: agentic → strict Pass^k (tasks where all k runs
    // passed / total tasks, spec §3.3); single-turn → the composite score as a
    // percent; an errored column → "Error". So the matrix is meaningful for any
    // collection, not just agentic ones.
    const pass = c.error
      ? "Error"
      : ag
        ? `${ag.tasks_passed}/${ag.tasks_total}`
        : fmtPct(c.toolcall?.composite);
    // Native FC pass^k is the parallel measurement; "N/A" when not run for this
    // model (unsupported backend / no `tools` capability) — never a fabricated 0.
    const nat = c.agentic_native_fc;
    // Native ran but EVERY task errored (broken template / infra) → no scored result. Show a
    // clear failure, not a misleading green "0/0". `top_error` is "None" here (nothing scored),
    // so the reason comes from `native_error_class`.
    const nativeAllErrored = nat != null && nat.tasks_total === 0 && (nat.tasks_errored ?? 0) > 0;
    const passKNative = c.error
      ? "Error"
      : nativeAllErrored
        ? NATIVE_ERROR_LABEL[nat!.native_error_class ?? "none"]
        : nat
          ? `${nat.tasks_passed}/${nat.tasks_total}`
          : "N/A";
    return {
      model: c.model,
      label: modelLabel(info ?? { name: c.model }),
      quant: info?.quantization || "—",
      passK: pass,
      passKNative,
      avgSteps: ag ? fmtNum(ag.avg_steps) : "—",
      avgStepsNative: c.error ? "Error" : nat ? fmtNum(nat.avg_steps) : "N/A",
      effort: ag ? fmtTokens(ag.avg_output_tokens_success) : "—",
      tokensPerTask: ag ? fmtTokens(ag.tokens_per_completed ?? null) : "—",
      // Schema resilience is agentic-only; null (no run hit a schema error) → "—".
      schemaResil: ag ? fmtPct(ag.schema_resilience) : "—",
      topError: c.error ? "Error" : ag ? TOP_ERROR_LABEL[ag.top_error] : "—",
      failures: ag?.failures ?? null,
      hasNative: nat != null,
      hasPrompt: ag != null || c.toolcall != null || c.error != null,
      effortNative: nat ? fmtTokens(nat.avg_output_tokens_success) : "N/A",
      tokensPerTaskNative: nat ? fmtTokens(nat.tokens_per_completed ?? null) : "N/A",
      schemaResilNative: nat ? fmtPct(nat.schema_resilience) : "—",
      topErrorNative: c.error ? "Error" : nativeAllErrored ? NATIVE_ERROR_LABEL[nat!.native_error_class ?? "none"] : nat ? TOP_ERROR_LABEL[nat.top_error] : "—",
      failuresNative: nat?.failures ?? null,
      composite: fmtPct(c.toolcall?.composite),
    };
  });
}

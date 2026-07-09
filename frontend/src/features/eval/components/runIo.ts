import type { TaskOutcome, TrajectoryStep } from "../../../shared/ipc/eval/batch";
import type { ToolTask } from "../../../shared/ipc/eval/registry";
import { agenticSystemPreview } from "../agenticPrompt";

/// Derives the "what went in / what came out" view-models for ONE run
/// (a (model, task) pair) from the already-cached outcome + trajectory. Pure — no
/// React, no store reads — so the RunIoModal stays a thin renderer and these rules
/// (especially the "no response" branches) are unit-testable in isolation.

const isAgenticCategory = (category: string) => category === "agentic" || category === "agent_loop";

/// The exact prompt handed to the model for the run. For a single-turn task this is
/// the real system message + user line captured in the trace; for an agentic task the
/// per-run prompt isn't persisted, so the system package is reconstructed from the
/// task's tool schemas. `note` is non-null whenever the shown prompt is NOT the
/// verbatim string the model received — a not-yet-run / errored single-turn task
/// (system built at run time) or an agentic task whose run-time tool set was widened
/// by synthetic decoys — so the modal never presents a reconstruction as captured.
export interface RunInputView {
  system: string;
  user: string;
  note: string | null;
}

export function buildRunInput(
  task: ToolTask,
  outcome: TaskOutcome | undefined,
  decoys?: number,
  steps?: TrajectoryStep[],
): RunInputView {
  // The real captured prompt — only single-turn traces persist it.
  if (outcome?.kind === "single") {
    return { system: outcome.trace.system_message, user: outcome.trace.user_prompt, note: null };
  }
  // Agentic (run, errored, or not-yet-run): the package is reconstructed from the
  // task's tools. Faithful UNLESS the run injected decoys the model also saw.
  if (isAgenticCategory(task.category)) {
    const note =
      decoys && decoys > 0
        ? `The model was also given ${decoys} synthetic decoy tool${decoys === 1 ? "" : "s"} at run time, not shown here.`
        : null;
    // Step 0 of a run carries the REAL user prompt (`TrajectoryStep.initial_prompt`) — a
    // generated task re-randomizes its entity ids per Pass^k run, so the static `task.prompt`
    // template is only right for the one seed that happens to match. The system package is
    // unaffected (tool schemas carry no entity ids) — only the user line needs the override.
    const realUser = steps?.find((s) => s.step_index === 0)?.initial_prompt;
    return { system: agenticSystemPreview(task), user: realUser ?? task.prompt, note };
  }
  // A single-turn task with no trace: its system message is built at run time, so we
  // can only show the user line verbatim. Distinguish errored-before-trace from never-run.
  const note =
    outcome?.kind === "error"
      ? "This run errored before its prompt was captured — the system message below is described, not the verbatim prompt."
      : "This task hasn’t been run yet — the system message below is described, not the verbatim prompt.";
  return { system: `System message is constructed at run time from ${task.tools.length} tool schema(s).`, user: task.prompt, note };
}

/// One Pass^k repetition's slice of the model output: its turns (each carrying the
/// model's raw text + any sandbox injection) and whether it reached the end state.
export interface RunOutputRun {
  runIndex: number;
  passed: boolean;
  steps: TrajectoryStep[];
}

/// What the model actually produced for the run. The `not_run` / `error` / `empty`
/// states are the "no response" indications the UI must surface explicitly rather
/// than render as a misleading blank.
export type RunOutputView =
  | { state: "not_run" }
  | { state: "error"; message: string }
  | { state: "empty"; reason: string }
  | { state: "single"; output: string }
  | { state: "agentic"; runs: RunOutputRun[] };

export function buildRunOutput(outcome: TaskOutcome | undefined, steps: TrajectoryStep[]): RunOutputView {
  // No terminal outcome yet — but an agentic task streams its turns live, so render
  // the in-flight trajectory rather than a false "not run yet" while it's running.
  if (!outcome) {
    return steps.length > 0 ? { state: "agentic", runs: groupByRun(steps) } : { state: "not_run" };
  }
  if (outcome.kind === "error") return { state: "error", message: outcome.message };
  if (outcome.kind === "single") {
    const out = outcome.trace.raw_output;
    if (out.trim() === "") {
      return { state: "empty", reason: "The model returned no output for this prompt." };
    }
    return { state: "single", output: out };
  }
  // Agentic: the model's output IS the streamed trajectory. No turns recorded means
  // the model never responded for this run (a hard stall / immediate failure).
  if (steps.length === 0) {
    return { state: "empty", reason: "The model produced no response — no turns were recorded for this run." };
  }
  return { state: "agentic", runs: groupByRun(steps) };
}

/// Bucket a flat trajectory (all k runs concatenated) into per-run groups, preserving
/// first-seen run order. A run passed iff its last turn reached the end state. (This
/// reflects the streamed trajectory tail; the authoritative aggregate Pass^k verdict
/// is the scoreboard's Result badge, computed from the report.)
function groupByRun(steps: TrajectoryStep[]): RunOutputRun[] {
  const order: number[] = [];
  const byIndex = new Map<number, TrajectoryStep[]>();
  for (const s of steps) {
    let arr = byIndex.get(s.run_index);
    if (!arr) {
      arr = [];
      byIndex.set(s.run_index, arr);
      order.push(s.run_index);
    }
    arr.push(s);
  }
  return order.map((runIndex) => {
    const rs = byIndex.get(runIndex)!;
    return { runIndex, passed: rs[rs.length - 1]?.kind === "end_state_reached", steps: rs };
  });
}

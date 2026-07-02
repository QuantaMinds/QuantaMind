import { useEffect, useState } from "react";
import type { LiveActivity } from "../../state/batchStore";
import type { StepKind } from "../../../../shared/ipc/eval/batch";

interface RunProgressProps {
  /// Task-level position across the whole batch.
  done: number;
  total: number;
  /// The current task + Pass^k run + turn, from the live event stream.
  live: LiveActivity;
  /// Run shape, so "Run 3/16" and "Step 4/10" show the denominators.
  k: number;
  maxSteps: number;
}

/// Human label per turn outcome — what the model just did this step. Keeps the
/// line truthful (e.g. a loop cap or forbidden call reads as such, never "working").
const KIND_LABEL: Record<StepKind, string> = {
  tool_call: "calling tools",
  tool_error: "handling tool error",
  unknown_tool: "called unknown tool",
  schema_error: "schema error",
  malformed_json: "malformed JSON",
  hallucinated_completion: "hallucinated completion",
  end_state_reached: "reached end state",
  infinite_loop: "loop cap hit",
  forbidden_call: "forbidden call",
  turn_timeout: "turn timed out",
  reported_in_prose: "answered in prose",
  foreign_dialect: "foreign tool dialect",
  empty_output: "empty output",
  truncated: "truncated (context full)",
  reasoning_overrun: "reasoning overrun",
};

function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(s / 60);
  return m > 0 ? `${m}m ${s % 60}s` : `${s}s`;
}

/// The live "Running…" detail line. Re-renders once per second (the elapsed clock)
/// AND whenever a new turn lands (the run/step counters), so even a model stuck
/// re-emitting the same tool calls for minutes visibly advances its step number —
/// the run reads as working, not hung.
export function RunProgress({ done, total, live, k, maxSteps }: RunProgressProps) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const pct = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
  const elapsedMs = live.startedAt != null ? now - live.startedAt : 0;
  const elapsed = live.startedAt != null ? formatElapsed(elapsedMs) : "0s";
  // Estimate the time left from the average per completed task (only meaningful once a task
  // has finished — the native pass streams no task_done, so its line shows elapsed only).
  const eta =
    live.startedAt != null && done > 0 && total > done ? formatElapsed((elapsedMs / done) * (total - done)) : null;

  const parts: string[] = [];
  // Name the pass — the native (tool-calling) pass runs first and is the slow one, so saying
  // so turns a long silent stretch into "Native pass, 45s elapsed" instead of a mystery.
  parts.push(live.native ? "Tool-Calling pass" : "Prompt-based pass");
  parts.push(live.taskId ? `Task ${live.taskId}` : "Preparing…");
  if (total > 0) parts.push(`${done}/${total} tasks`);
  if (live.runIndex != null) parts.push(`Run ${live.runIndex + 1}/${k}`);
  if (live.stepIndex != null) parts.push(`Step ${live.stepIndex + 1}/${maxSteps}`);
  if (live.stepKind) parts.push(KIND_LABEL[live.stepKind]);
  parts.push(`${elapsed} elapsed`);
  if (eta) parts.push(`~${eta} left`);

  return (
    <div style={{ padding: "10px 16px", borderBottom: "1px solid #e2e8f0" }} data-testid="scoreboard-progress">
      <div
        style={{ fontSize: 11, color: "#475569", fontFamily: "'JetBrains Mono', monospace", marginBottom: 6 }}
        data-testid="scoreboard-progress-detail"
      >
        {parts.join(" · ")}
      </div>
      <div style={{ height: 4, background: "#f1f5f9", borderRadius: 2 }}>
        <div style={{ height: 4, width: `${pct}%`, background: "#2563eb", borderRadius: 2, transition: "width 120ms" }} />
      </div>
    </div>
  );
}

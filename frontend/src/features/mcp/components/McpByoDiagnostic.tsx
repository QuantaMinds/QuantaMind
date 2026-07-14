import { useState } from "react";
import { runMcpByo, type ByoRunResult } from "../../../shared/ipc/mcp/run";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import type { McpByoTaskDef } from "../state/mcpStore";

/// The diagnostic surface for ONE Bring-Your-Own task. Runs the model against the
/// user's real server and shows what it CAN do — a live per-call trace + schema-
/// valid rate + model/config/server attribution. Deliberately NO pass/fail verdict:
/// there is no answer key, so a readiness claim would be dishonest.
const ATTR_COLOR: Record<string, string> = {
  success: "#10b981",
  model: "#f59e0b",
  config: "#3b82f6",
  server: "#ef4444",
};

export function McpByoDiagnostic({ task, onBack }: { task: McpByoTaskDef; onBack: () => void }) {
  const model = useSelectedModelStore((s) => s.selectedModels[0]);
  const [status, setStatus] = useState<"idle" | "running" | "done" | "error">("idle");
  const [result, setResult] = useState<ByoRunResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    if (!model) return;
    setStatus("running");
    setError(null);
    setResult(null);
    try {
      const r = await runMcpByo(model.name, model.backend, task.serverId, task.instruction);
      setResult(r);
      setStatus("done");
    } catch (e) {
      setError(String(e));
      setStatus("error");
    }
  };

  const pct = result ? Math.round(result.schema_valid_rate * 100) : 0;

  return (
    <div className="flex flex-col gap-3 min-w-0">
      <div className="flex items-center gap-3">
        <button type="button" className="text-xs underline opacity-80" onClick={onBack}>
          ← back
        </button>
        <div className="font-semibold">🔧 {task.name}</div>
        <span className="text-xs opacity-60">Diagnostic · {task.serverId}</span>
      </div>

      <div className="rounded-lg border border-neutral-700 p-3 text-sm">
        <div className="opacity-70">{task.instruction}</div>
        <div className="mt-1 text-xs">
          Model:{" "}
          {model ? (
            <b>
              {model.name} · {model.backend}
            </b>
          ) : (
            <span className="text-amber-400">none selected</span>
          )}
        </div>
        <button
          type="button"
          className="mt-2 rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900 disabled:opacity-40"
          disabled={!model || status === "running"}
          onClick={() => void run()}
        >
          {status === "running" ? "Running…" : "Run diagnostic"}
        </button>
      </div>

      {error && <div className="rounded border border-red-500/50 bg-red-500/10 p-2 text-xs text-red-300">{error}</div>}

      {result && (
        <div className="flex flex-col gap-2">
          <div className="rounded-lg border border-neutral-700 p-3 text-sm">
            <div>
              Calls: <b>{result.total_calls}</b> · schema-valid: <b>{result.schema_valid}/{result.total_calls}</b> ({pct}%)
            </div>
            <div className="mt-1 text-xs opacity-80">
              faults → model: {result.model_faults} · config: {result.config_faults} · server: {result.server_faults} ·
              successes: {result.successes}
            </div>
            <div className="mt-1 text-xs italic opacity-50">No pass/fail — no answer key.</div>
          </div>

          <div className="flex flex-col gap-1">
            {result.calls.map((c, i) => (
              <div key={i} className="rounded border border-neutral-700 p-2 text-xs">
                <div className="flex items-center gap-2">
                  <span>{c.schema_valid ? "✓" : "✕"}</span>
                  <span className="font-mono">{c.tool}</span>
                  <span style={{ color: ATTR_COLOR[c.attribution] ?? "#94a3b8" }}>{c.attribution}</span>
                </div>
                {c.detail && <div className="mt-0.5 opacity-60">{c.detail}</div>}
              </div>
            ))}
          </div>

          {result.assistant_text && (
            <div className="rounded-lg border border-neutral-700 p-3 text-xs opacity-80 whitespace-pre-wrap">
              {result.assistant_text}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

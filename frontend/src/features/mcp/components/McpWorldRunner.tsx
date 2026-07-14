import { useState } from "react";
import { useMcpStore } from "../state/mcpStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { runMcpWorldTask, type McpRunResult } from "../../../shared/ipc/mcp/run";

/// Screens 5–6 for the controlled world: Run a built task against the globally
/// selected model, then show the pass^k verdict + why (grading the world, not
/// the words). Fake-done shows up as a low pass^k with "expected present but
/// missing" failures.
export function McpWorldRunner() {
  const tasks = useMcpStore((s) => s.tasks);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);
  const [results, setResults] = useState<Record<string, McpRunResult | "running" | { error: string }>>({});

  const run = async (name: string) => {
    const task = tasks.find((t) => t.name === name);
    if (!task || !model) return;
    setResults((r) => ({ ...r, [name]: "running" }));
    try {
      const res = await runMcpWorldTask(model.name, model.backend, task);
      setResults((r) => ({ ...r, [name]: res }));
    } catch (e) {
      setResults((r) => ({ ...r, [name]: { error: String(e) } }));
    }
  };

  if (tasks.length === 0) return null;

  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-base font-semibold">Run &amp; score</h3>
      {!model && <p className="text-sm text-amber-400">Select a model in the header to run.</p>}
      {tasks.map((t) => {
        const res = results[t.name];
        return (
          <div key={t.name} className="rounded-lg border border-neutral-700 p-2">
            <div className="flex items-center gap-3">
              <span className="text-sm">🌍 {t.name}</span>
              <span className="text-xs opacity-60">{t.world.type} · pass^{t.k}</span>
              <button
                type="button"
                className="ml-auto rounded bg-neutral-200 px-3 py-1 text-xs text-neutral-900 disabled:opacity-40"
                disabled={!model || res === "running"}
                onClick={() => void run(t.name)}
              >
                {res === "running" ? "running…" : "Run ▶"}
              </button>
            </div>
            {res && res !== "running" && <Verdict res={res} />}
          </div>
        );
      })}
    </div>
  );
}

function Verdict({ res }: { res: McpRunResult | { error: string } }) {
  if ("error" in res) return <div className="mt-2 text-xs text-red-400">✗ {res.error}</div>;
  const label = res.ready ? "READY" : res.passes > 0 ? "CONDITIONAL" : "NOT READY";
  const color = res.ready ? "text-emerald-400" : res.passes > 0 ? "text-amber-400" : "text-red-400";
  return (
    <div className="mt-2 text-xs">
      <div className={color}>
        Verdict: {label} · passed {res.passes}/{res.k} · pass^{res.k} = {res.pass_rate.toFixed(2)}
      </div>
      {res.failures.length > 0 && (
        <ul className="mt-1 opacity-70">
          {res.failures.slice(0, 3).map((f, i) => (
            <li key={i}>• run failed: {f.join("; ")}</li>
          ))}
          {res.failures.length > 3 && <li>• …and {res.failures.length - 3} more</li>}
        </ul>
      )}
      {!res.ready && res.passes === 0 && (
        <div className="mt-1 opacity-70">→ Graded on the world, not the model's words (fake-done shows here).</div>
      )}
    </div>
  );
}

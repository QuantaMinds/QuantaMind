import { useState } from "react";
import { useMcpStore } from "../state/mcpStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { runMcpByo, type ByoRunResult } from "../../../shared/ipc/mcp/run";

/// Bring-Your-Own: run the model once against a real server and grade the CALLS
/// (schema-valid rate + whose-fault attribution). No world, no task verdict —
/// this is the honest Track A: format + attribution, not pass/fail.
export function McpByoRunner() {
  const servers = useMcpStore((s) => s.servers);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);
  const [serverId, setServerId] = useState("");
  const [instruction, setInstruction] = useState("");
  const [state, setState] = useState<ByoRunResult | "running" | { error: string } | null>(null);

  const run = async () => {
    if (!model || !serverId || !instruction.trim()) return;
    setState("running");
    try {
      setState(await runMcpByo(model.name, model.backend, serverId, instruction.trim()));
    } catch (e) {
      setState({ error: String(e) });
    }
  };

  const input = "rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm";
  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-base font-semibold">Run against your tools</h3>
      <p className="text-xs opacity-60">
        We grade tool-call correctness + whose-fault attribution. No task-completion verdict — no answer key.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <select className={input} value={serverId} onChange={(e) => setServerId(e.target.value)}>
          <option value="">select server…</option>
          {servers.map((s) => (
            <option key={s.id} value={s.id}>{s.id}</option>
          ))}
        </select>
        <input className={`${input} flex-1`} placeholder="instruction (e.g. list the files in the project)" value={instruction} onChange={(e) => setInstruction(e.target.value)} />
        <button type="button" className="rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900 disabled:opacity-40" disabled={!model || !serverId || state === "running"} onClick={() => void run()}>
          {state === "running" ? "running…" : "Run ▶"}
        </button>
      </div>
      {!model && <p className="text-sm text-amber-400">Select a model in the header to run.</p>}
      {state && state !== "running" && <ByoResult state={state} />}
    </div>
  );
}

function ByoResult({ state }: { state: ByoRunResult | { error: string } }) {
  if ("error" in state) return <div className="text-xs text-red-400">✗ {state.error}</div>;
  return (
    <div className="rounded-lg border border-neutral-700 p-2 text-xs">
      <div>
        schema-valid rate: <b>{(state.schema_valid_rate * 100).toFixed(0)}%</b> ({state.schema_valid}/{state.total_calls})
      </div>
      <div className="opacity-80">
        attribution — model: {state.model_faults} · config: {state.config_faults} · server: {state.server_faults} · ok: {state.successes}
      </div>
      <ul className="mt-1 opacity-70">
        {state.calls.map((c, i) => (
          <li key={i}>
            {c.schema_valid ? "✓" : "✗"} {c.tool} → {c.attribution}
            {c.detail && c.attribution !== "success" ? ` (${c.detail})` : ""}
          </li>
        ))}
      </ul>
    </div>
  );
}

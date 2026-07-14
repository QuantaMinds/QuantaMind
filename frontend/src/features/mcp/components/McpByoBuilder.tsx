import { useState } from "react";
import { useMcpStore } from "../state/mcpStore";

/// Bring-Your-Own authoring: name + instruction + which connected server. Save
/// writes one `McpByoTaskDef` into the sidebar and collapses the panel — the same
/// Save→collapse flow as the Test-World builder. There is NO oracle here: a BYO
/// task is diagnostic-only (run one from the sidebar → live trace, no verdict).
export function McpByoBuilder() {
  const servers = useMcpStore((s) => s.servers);
  const addByoTask = useMcpStore((s) => s.addByoTask);
  const [name, setName] = useState("");
  const [instruction, setInstruction] = useState("");
  const [serverId, setServerId] = useState("");

  const input = "rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm";
  const sec = "rounded-lg border border-neutral-700 p-3 flex flex-col gap-2";
  const chosen = serverId || servers[0]?.id || "";
  const canSave = name.trim() && instruction.trim() && chosen;

  const save = () => {
    if (!canSave) return;
    addByoTask({ name: name.trim(), instruction: instruction.trim(), serverId: chosen });
    setName("");
    setInstruction("");
  };

  return (
    <div className="flex flex-col gap-3">
      <div className={sec}>
        <div className="text-xs font-semibold uppercase opacity-60">Bring your own server</div>
        <input className={input} placeholder="task name" value={name} onChange={(e) => setName(e.target.value)} />
        <input
          className={input}
          placeholder="Instruction (e.g. list the files you can see, then read README.md)"
          value={instruction}
          onChange={(e) => setInstruction(e.target.value)}
        />
        <label className="text-xs opacity-70">Run against</label>
        {servers.length === 0 ? (
          <div className="text-xs italic text-amber-400">Connect a server above first.</div>
        ) : (
          <select className={input} value={chosen} onChange={(e) => setServerId(e.target.value)}>
            {servers.map((sv) => (
              <option key={sv.id} value={sv.id}>
                {sv.id}
              </option>
            ))}
          </select>
        )}
        <div className="text-xs opacity-50">
          No answer key → diagnostic only: schema-valid rate + model/config/server attribution + live trace. No pass/fail.
        </div>
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          className="rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900 disabled:opacity-40"
          disabled={!canSave}
          onClick={save}
        >
          Save task
        </button>
        <span className="text-xs opacity-50">Saved tasks appear in the sidebar; run one for its diagnostic.</span>
      </div>
    </div>
  );
}

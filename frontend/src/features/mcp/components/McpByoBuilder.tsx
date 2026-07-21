import { useEffect, useState } from "react";
import { useMcpStore } from "../state/mcpStore";

/// Bring-Your-Own authoring: name + instruction + which connected server. Save
/// writes one `McpByoTaskDef` into the sidebar and collapses the panel — the same
/// Save→collapse flow as the Test-World builder. There is NO oracle here: a BYO
/// task is diagnostic-only (run one from the sidebar → live trace, no verdict).
export function McpByoBuilder() {
  const servers = useMcpStore((s) => s.servers);
  const addByoTask = useMcpStore((s) => s.addByoTask);
  const editingByo = useMcpStore((s) => s.editingByo);
  const [name, setName] = useState("");
  const [instruction, setInstruction] = useState("");
  const [serverId, setServerId] = useState("");

  // Editing an existing task (the sidebar pencil) → pre-fill from it; Save (same name) replaces it.
  useEffect(() => {
    if (editingByo) {
      setName(editingByo.name);
      setInstruction(editingByo.instruction);
      setServerId(editingByo.serverId);
    }
  }, [editingByo]);

  const input = "rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm";
  const sec = "rounded-lg border border-neutral-700 p-3 flex flex-col gap-2";
  const chosen = serverId || servers[0]?.id || "";
  // Name is optional — a task only needs an instruction + a connected server. Fall
  // back to the instruction text so the sidebar row is still readable.
  const finalName = name.trim() || instruction.trim().slice(0, 32);
  const canSave = Boolean(instruction.trim() && chosen);

  const save = () => {
    if (!canSave) return;
    addByoTask({ name: finalName, instruction: instruction.trim(), serverId: chosen });
    setName("");
    setInstruction("");
  };

  return (
    <div className="flex flex-col gap-3">
      <div className={sec}>
        <div className="text-xs font-semibold uppercase opacity-60">Bring your own server</div>
        <input className={input} placeholder="task name (optional)" value={name} onChange={(e) => setName(e.target.value)} />
        <label className="text-xs opacity-70">
          Instruction <span className="text-red-500">*</span>
        </label>
        <input
          className={input}
          placeholder="e.g. List all the tables in the database."
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
        <ByoExecuteToggle />
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          className="rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900 disabled:opacity-40"
          disabled={!canSave}
          onClick={save}
        >
          {editingByo ? "Update task" : "Save task"}
        </button>
        {canSave ? (
          <span className="text-xs opacity-50">Saved tasks appear in the sidebar; run one for its diagnostic.</span>
        ) : (
          <span className="text-xs text-amber-400">
            {!chosen ? "Connect a server above first." : "Enter an instruction to enable Save."}
          </span>
        )}
      </div>
    </div>
  );
}

/// The run-level safety opt-in for Bring-Your-Own runs. Real tools are deny-by-default;
/// this is the explicit approval that lets the model's calls actually execute. Shown both
/// in the builder AND in the collapsed run view (issue #192) so it's reachable at run time.
export function ByoExecuteToggle() {
  const allowByoExecute = useMcpStore((s) => s.allowByoExecute);
  const setAllowByoExecute = useMcpStore((s) => s.setAllowByoExecute);
  return (
    <label className="flex items-start gap-2 rounded border border-amber-500/40 bg-amber-500/5 p-2 text-xs">
      <input
        type="checkbox"
        className="mt-0.5"
        checked={allowByoExecute}
        onChange={(e) => setAllowByoExecute(e.target.checked)}
        data-testid="byo-allow-execute"
      />
      <span>
        <span className="font-semibold text-amber-300">Allow tool execution</span> — run the model's calls against your{" "}
        <em>real</em> server. Off by default: calls are graded but <em>not run</em>. Only enable for a server you've
        scoped to a throwaway target (e.g. a disposable directory), since the model's calls will actually execute.
      </span>
    </label>
  );
}

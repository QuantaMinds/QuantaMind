import { useState } from "react";
import { useMcpStore, type McpFsSeedFile, type McpTaskDef } from "../state/mcpStore";

/// Screen 4 — the guided builder. Three sections that ARE the task JSON: Task
/// (instruction) · Set up the world (seed) · Check the result (oracle). Save
/// writes exactly one `McpTaskDef` — the same format the JSON-upload path takes,
/// so the builder and upload converge on one engine.

export interface BuilderForm {
  name: string;
  instruction: string;
  k: number;
  worldType: "fs" | "db";
  files: McpFsSeedFile[];
  setupSql: string;
  present: string; // newline-separated paths
  absent: string;
  content: string; // "path :: substring" per line
  assertEq: string; // "SELECT … :: expected" per line
}

const lines = (s: string) => s.split("\n").map((x) => x.trim()).filter(Boolean);
const pairs = (s: string): [string, string][] =>
  lines(s).map((l) => {
    const i = l.indexOf("::");
    return i < 0 ? [l, ""] : [l.slice(0, i).trim(), l.slice(i + 2).trim()];
  });

/// Pure form → task JSON (unit-tested).
export function toTaskDef(f: BuilderForm): McpTaskDef {
  const world: McpTaskDef["world"] =
    f.worldType === "fs"
      ? { type: "fs", files: f.files.filter((x) => x.path.trim()) }
      : { type: "db", setupSql: f.setupSql };
  const oracle: McpTaskDef["oracle"] =
    f.worldType === "fs"
      ? { assert_present: lines(f.present), assert_absent: lines(f.absent), assert_content: pairs(f.content) }
      : { assert_eq: pairs(f.assertEq) };
  return { name: f.name.trim(), instruction: f.instruction.trim(), world, oracle, k: f.k };
}

const EMPTY: BuilderForm = {
  name: "",
  instruction: "",
  k: 10,
  worldType: "fs",
  files: [{ path: "", content: "" }],
  setupSql: "",
  present: "",
  absent: "",
  content: "",
  assertEq: "",
};

export function McpTaskBuilder() {
  const addTask = useMcpStore((s) => s.addTask);
  const [f, setF] = useState<BuilderForm>(EMPTY);
  const up = (patch: Partial<BuilderForm>) => setF((cur) => ({ ...cur, ...patch }));

  const save = () => {
    if (!f.name.trim() || !f.instruction.trim()) return;
    addTask(toTaskDef(f));
    setF(EMPTY);
  };

  const sec = "rounded-lg border border-neutral-700 p-3 flex flex-col gap-2";
  const input = "rounded border border-neutral-600 bg-transparent px-2 py-1 text-sm";

  return (
    <div className="flex flex-col gap-3">
      {/* ── Task ── */}
      <div className={sec}>
        <div className="text-xs font-semibold uppercase opacity-60">Task</div>
        <input className={input} placeholder="task name" value={f.name} onChange={(e) => up({ name: e.target.value })} />
        <input className={input} placeholder="Instruction (e.g. create result.txt with DONE)" value={f.instruction} onChange={(e) => up({ instruction: e.target.value })} />
      </div>

      {/* ── Set up the world ── */}
      <div className={sec}>
        <div className="text-xs font-semibold uppercase opacity-60">Set up the world (the starting state)</div>
        <div className="flex gap-4 text-sm">
          <label className="flex items-center gap-1">
            <input type="radio" checked={f.worldType === "fs"} onChange={() => up({ worldType: "fs" })} /> Temp folder
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" checked={f.worldType === "db"} onChange={() => up({ worldType: "db" })} /> Temp database
          </label>
        </div>
        {f.worldType === "fs" ? (
          <div className="flex flex-col gap-1">
            {f.files.map((file, i) => (
              <div key={i} className="flex gap-2">
                <input className={`${input} w-40`} placeholder="filename" value={file.path} onChange={(e) => up({ files: f.files.map((x, j) => (j === i ? { ...x, path: e.target.value } : x)) })} />
                <input className={`${input} flex-1`} placeholder="contents" value={file.content} onChange={(e) => up({ files: f.files.map((x, j) => (j === i ? { ...x, content: e.target.value } : x)) })} />
                <button type="button" className="px-2 text-sm" onClick={() => up({ files: f.files.filter((_, j) => j !== i) })}>−</button>
              </div>
            ))}
            <button type="button" className="self-start text-xs opacity-70" onClick={() => up({ files: [...f.files, { path: "", content: "" }] })}>+ add file</button>
            <div className="text-xs opacity-50">Scope server to: filesystem (auto-confined to this temp folder 🔒)</div>
          </div>
        ) : (
          <textarea className={`${input} font-mono`} rows={3} placeholder="setup SQL (CREATE TABLE …; INSERT …)" value={f.setupSql} onChange={(e) => up({ setupSql: e.target.value })} />
        )}
      </div>

      {/* ── Check the result (oracle) ── */}
      <div className={sec}>
        <div className="text-xs font-semibold uppercase opacity-60">Check the result (the answer key)</div>
        {f.worldType === "fs" ? (
          <>
            <label className="text-xs opacity-70">Files that must be PRESENT (one per line)</label>
            <textarea className={`${input} font-mono`} rows={2} value={f.present} onChange={(e) => up({ present: e.target.value })} />
            <label className="text-xs opacity-70">Files that must be ABSENT (one per line)</label>
            <textarea className={`${input} font-mono`} rows={2} value={f.absent} onChange={(e) => up({ absent: e.target.value })} />
            <label className="text-xs opacity-70">Content checks — "path :: substring" per line</label>
            <textarea className={`${input} font-mono`} rows={2} value={f.content} onChange={(e) => up({ content: e.target.value })} />
          </>
        ) : (
          <>
            <label className="text-xs opacity-70">Query assertions — "SELECT … :: expected" per line</label>
            <textarea className={`${input} font-mono`} rows={3} value={f.assertEq} onChange={(e) => up({ assertEq: e.target.value })} />
          </>
        )}
      </div>

      <div className="flex items-center gap-3">
        <label className="text-sm">
          Runs (pass^k):{" "}
          <select className={input} value={f.k} onChange={(e) => up({ k: Number(e.target.value) })}>
            {[1, 3, 5, 8, 10, 16].map((n) => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
        </label>
        <button type="button" className="rounded bg-neutral-200 px-3 py-1 text-sm text-neutral-900" onClick={save}>
          Save task
        </button>
      </div>
    </div>
  );
}

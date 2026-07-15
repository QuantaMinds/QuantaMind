import { useState } from "react";
import { McpConnectPanel } from "./McpConnectPanel";
import { McpTaskBuilder } from "./McpTaskBuilder";
import { McpByoBuilder } from "./McpByoBuilder";
import { useMcpStore } from "../state/mcpStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";

type Track = "controlled" | "byo";

/// The CENTER of the Test page when MCP is the active source. Connect real
/// servers, then author a task one of two ways — QuantaMind Test World (seeded,
/// answer-key scored via Run Batch) or Bring-Your-Own (your server, diagnostic
/// only). Both Save→collapse and list their tasks in the sidebar. Model + params
/// come from the main controls.
export function McpCenterPanel() {
  const collapsed = useMcpStore((s) => s.builderCollapsed);
  const setCollapsed = useMcpStore((s) => s.setBuilderCollapsed);
  const taskCount = useMcpStore((s) => s.tasks.length);
  const byoCount = useMcpStore((s) => s.byoTasks.length);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);
  const [track, setTrack] = useState<Track>("controlled");

  const total = taskCount + byoCount;
  if (collapsed && total > 0) {
    return (
      <div className="flex flex-col gap-3 min-w-0">
        <div className="rounded-lg border border-neutral-700 p-3 text-sm">
          ✓ {total} MCP task{total === 1 ? "" : "s"} saved — they're in the sidebar.
          {" "}
          Hit <b>Run Batch</b> to run them
          {taskCount > 0 && byoCount === 0 && " — Test-World tasks are scored (pass^k)"}
          {byoCount > 0 && taskCount === 0 && " — Bring-Your-Own tasks stream a diagnostic (schema-valid, no verdict)"}.
          <button type="button" className="ml-3 text-xs underline opacity-80" onClick={() => setCollapsed(false)}>
            + Add another
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 min-w-0">
      <TrackSelector track={track} onSelect={setTrack} />

      <div>
        <h2 className="text-lg font-semibold">Connect your MCP tools</h2>
        <p className="text-sm opacity-70">Point at real MCP servers, then author a task to run.</p>
        <div className="mt-1 text-xs">
          Model: {model ? <b>{model.name}</b> : <span className="text-amber-400">none selected</span>}
          {model && <span className="opacity-60"> · {model.backend}</span>}
          <span className="opacity-50"> (from the global header · iterations + decoy from Run Params)</span>
        </div>
      </div>

      <McpConnectPanel />

      <div className="flex flex-col gap-2">
        {track === "controlled" ? (
          <>
            <h3 className="text-base font-semibold">QuantaMind Test World</h3>
            <McpTaskBuilder />
            <p className="text-xs opacity-60">Saved tasks appear in the sidebar; Run Batch scores them like Built-In.</p>
          </>
        ) : (
          <>
            <h3 className="text-base font-semibold">Bring-Your-Own</h3>
            <McpByoBuilder />
          </>
        )}
      </div>
    </div>
  );
}

/// The two authoring doors. They look DIFFERENT on purpose — Bring-Your-Own carries
/// no task-completion verdict (seeing ≠ scoring; no answer key).
function TrackSelector({ track, onSelect }: { track: Track; onSelect: (t: Track) => void }) {
  const base = "flex-1 rounded-lg border p-3 text-left transition";
  return (
    <div className="flex gap-3">
      <button
        type="button"
        onClick={() => onSelect("controlled")}
        className={`${base} ${track === "controlled" ? "border-emerald-500 bg-emerald-500/10" : "border-neutral-700"}`}
      >
        <div className="font-medium">QuantaMind Test World</div>
        <div className="text-xs opacity-70">We seed a controlled sandbox → ✅ full pass/fail task verdict (pass^k).</div>
      </button>
      <button
        type="button"
        onClick={() => onSelect("byo")}
        className={`${base} ${track === "byo" ? "border-amber-500 bg-amber-500/10" : "border-neutral-700"}`}
      >
        <div className="font-medium">Bring-Your-Own</div>
        <div className="text-xs opacity-70">
          Point at your real tools → ⚠️ diagnostic only: schema-valid rate + attribution + live trace.
          <span className="italic"> No pass/fail — no answer key.</span>
        </div>
      </button>
    </div>
  );
}

import { useEffect, useState } from "react";
import { McpConnectPanel } from "./McpConnectPanel";
import { McpTaskBuilder } from "./McpTaskBuilder";
import { McpByoBuilder, ByoExecuteToggle } from "./McpByoBuilder";
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
  const editingByo = useMcpStore((s) => s.editingByo);
  const setEditingByo = useMcpStore((s) => s.setEditingByo);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);
  const [track, setTrack] = useState<Track>("controlled");

  // Editing a BYO task (sidebar pencil) forces the Bring-Your-Own door open.
  useEffect(() => {
    if (editingByo) setTrack("byo");
  }, [editingByo]);

  const total = taskCount + byoCount;
  // Collapsed with tasks saved: render nothing here so the result panels (and the sidebar's
  // task list + "+ Add MCP task") own the screen. The old "N tasks saved" banner was redundant.
  // Exception: when BYO tasks exist, keep the "Allow tool execution" opt-in reachable at run
  // time — otherwise the deny-by-default gate (issue #192) can't be approved after collapse.
  if (collapsed && total > 0) {
    return byoCount > 0 ? (
      <div className="mb-2" data-testid="byo-execute-collapsed">
        <ByoExecuteToggle />
      </div>
    ) : null;
  }

  return (
    <div className="flex flex-col gap-4 min-w-0">
      {/* Once tasks exist, the builder can be dismissed back to the collapsed summary so it
          doesn't sit on top of the result panels (Simulator/Evaluator/Model Results). */}
      {total > 0 && (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => {
              setEditingByo(null);
              setCollapsed(true);
            }}
            className="rounded border border-neutral-600 px-3 py-1 text-xs opacity-80"
          >
            Cancel — back to results
          </button>
        </div>
      )}
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
        <div className="text-xs opacity-70">We seed a controlled sandbox → full pass/fail task verdict (pass^k).</div>
      </button>
      <button
        type="button"
        onClick={() => onSelect("byo")}
        className={`${base} ${track === "byo" ? "border-amber-500 bg-amber-500/10" : "border-neutral-700"}`}
      >
        <div className="font-medium">Bring-Your-Own</div>
        <div className="text-xs opacity-70">
          Point at your real tools → diagnostic only: schema-valid rate + attribution + live trace.
          <span className="italic"> No pass/fail — no answer key.</span>
        </div>
      </button>
    </div>
  );
}

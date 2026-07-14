import { McpConnectPanel } from "./McpConnectPanel";
import { McpTaskBuilder } from "./McpTaskBuilder";
import { useMcpStore } from "../state/mcpStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";

/// The CENTER of the Test page when MCP is the active source. Connect real
/// servers, then define a task to score in a controlled world. Once a task is
/// saved the screen collapses (tasks live in the sidebar; Run Batch scores them
/// like Built-In). Model + params (iterations, decoy) come from the main controls.
export function McpCenterPanel() {
  const collapsed = useMcpStore((s) => s.builderCollapsed);
  const setCollapsed = useMcpStore((s) => s.setBuilderCollapsed);
  const taskCount = useMcpStore((s) => s.tasks.length);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);

  if (collapsed && taskCount > 0) {
    return (
      <div className="flex flex-col gap-3 min-w-0">
        <div className="rounded-lg border border-neutral-700 p-3 text-sm">
          ✓ {taskCount} MCP task{taskCount === 1 ? "" : "s"} saved — they're in the sidebar. Select one and
          hit <b>Run Batch</b> to score it through the pipeline.
          <button type="button" className="ml-3 text-xs underline opacity-80" onClick={() => setCollapsed(false)}>
            + Add another
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 min-w-0">
      <div>
        <h2 className="text-lg font-semibold">Connect your MCP tools</h2>
        <p className="text-sm opacity-70">
          Point at real MCP servers, then define a task to score in a controlled world.
        </p>
        <div className="mt-1 text-xs">
          Model:{" "}
          {model ? <b>{model.name}</b> : <span className="text-amber-400">none selected</span>}
          {model && <span className="opacity-60"> · {model.backend}</span>}
          <span className="opacity-50"> (from the global header · iterations + decoy from Run Params)</span>
        </div>
      </div>

      <McpConnectPanel />

      <div className="flex flex-col gap-2">
        <h3 className="text-base font-semibold">Define the test</h3>
        <McpTaskBuilder />
        <p className="text-xs opacity-60">Saved tasks appear in the sidebar; Run Batch scores them like Built-In.</p>
      </div>
    </div>
  );
}

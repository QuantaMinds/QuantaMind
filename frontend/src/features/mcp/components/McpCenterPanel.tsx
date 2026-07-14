import { McpConnectPanel } from "./McpConnectPanel";
import { McpTaskBuilder } from "./McpTaskBuilder";
import { McpByoRunner } from "./McpByoRunner";
import { useMcpStore } from "../state/mcpStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";

/// The CENTER of the Test page when MCP is the active source. Connect real
/// servers, then either build+run a controlled-world task (full pass/fail
/// scoring) or run against your own live tools (format + attribution only). The
/// model/endpoint come from the global header — no separate picker.
export function McpCenterPanel() {
  const mode = useMcpStore((s) => s.mode);
  const model = useSelectedModelStore((s) => s.selectedModels[0]);

  return (
    <div className="flex flex-col gap-4 min-w-0">
      <div>
        <h2 className="text-lg font-semibold">Connect your MCP tools</h2>
        <p className="text-sm opacity-70">
          Point at real MCP servers, then build a task in a controlled world (full pass/fail scoring)
          or run against your own live tools (format + attribution only).
        </p>
        <div className="mt-1 text-xs">
          Model:{" "}
          {model ? (
            <b>{model.name}</b>
          ) : (
            <span className="text-amber-400">none selected</span>
          )}
          {model && <span className="opacity-60"> · {model.backend}</span>}
          <span className="opacity-50"> (from the global header)</span>
        </div>
      </div>

      <McpConnectPanel />

      {mode === "controlled" ? (
        <>
          <div className="flex flex-col gap-2">
            <h3 className="text-base font-semibold">Define the test</h3>
            <McpTaskBuilder />
            <p className="text-xs opacity-60">
              Saved tasks appear in the sidebar. Hit <b>Run Batch</b> to score them through the same
              pipeline as Built-In — Simulator, Evaluator, Model Result, Audit, and Agent Report.
            </p>
          </div>
        </>
      ) : (
        <McpByoRunner />
      )}
    </div>
  );
}

import { McpConnectPanel } from "./McpConnectPanel";
import { McpTaskBuilder } from "./McpTaskBuilder";
import { useMcpStore } from "../state/mcpStore";

/// The CENTER of the Test page when the MCP source is active. Screen 2 —
/// "Connect your MCP tools": before any task, the user points at real servers
/// and sees "N tools discovered" (the doctor/preflight moment). The sidebar
/// meanwhile shows only the list of saved MCP tasks.
///
/// Later slices add Screen 3 (the three doors: build / upload / template),
/// Screen 4 (the guided world/oracle builder that writes the one task JSON),
/// Screen 5 (the live trace), and Screen 6 (verdict + attribution).
export function McpCenterPanel() {
  const mode = useMcpStore((s) => s.mode);
  return (
    <div className="flex flex-col gap-4 min-w-0">
      <div>
        <h2 className="text-lg font-semibold">Connect your MCP tools</h2>
        <p className="text-sm opacity-70">
          Point at real MCP servers, then build a task in a controlled world (full pass/fail scoring)
          or run against your own live tools (format + attribution only).
        </p>
      </div>
      <McpConnectPanel />
      {mode === "controlled" ? (
        <div className="flex flex-col gap-2">
          <h3 className="text-base font-semibold">Define the test</h3>
          <McpTaskBuilder />
        </div>
      ) : (
        <div className="rounded-lg border border-amber-600/40 bg-amber-500/5 p-3 text-sm">
          Bring-Your-Own live: we check tool-call correctness + whose-fault attribution. For full
          pass/fail task scoring, switch to a QuantaMind controlled world.
        </div>
      )}
    </div>
  );
}

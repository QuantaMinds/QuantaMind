import { McpConnectPanel } from "./McpConnectPanel";

/// The CENTER of the Test page when the MCP source is active. Screen 2 —
/// "Connect your MCP tools": before any task, the user points at real servers
/// and sees "N tools discovered" (the doctor/preflight moment). The sidebar
/// meanwhile shows only the list of saved MCP tasks.
///
/// Later slices add Screen 3 (the three doors: build / upload / template),
/// Screen 4 (the guided world/oracle builder that writes the one task JSON),
/// Screen 5 (the live trace), and Screen 6 (verdict + attribution).
export function McpCenterPanel() {
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
    </div>
  );
}

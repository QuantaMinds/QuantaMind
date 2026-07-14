import { invoke } from "@tauri-apps/api/core";
import type { McpTaskDef } from "../../../features/mcp/state/mcpStore";
import type { ToolTask } from "../eval/registry";

/// Convert built MCP tasks (world + oracle) into eval `ToolTask`s so they run
/// through the SAME batch pipeline as Built-In collections (Stage 2/3).
export async function buildMcpTasks(tasks: McpTaskDef[]): Promise<ToolTask[]> {
  return (await invoke("build_mcp_tasks", { tasks })) as ToolTask[];
}

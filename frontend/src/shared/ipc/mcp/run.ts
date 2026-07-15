import { invoke } from "@tauri-apps/api/core";
import type { BackendKind } from "../models/storage";
import type { McpTaskDef, McpByoTaskDef } from "../../../features/mcp/state/mcpStore";
import type { ToolTask } from "../eval/registry";

/// Convert built MCP tasks (world + oracle) into eval `ToolTask`s so they run
/// through the SAME batch pipeline as Built-In collections (Stage 2/3).
export async function buildMcpTasks(tasks: McpTaskDef[]): Promise<ToolTask[]> {
  return (await invoke("build_mcp_tasks", { tasks })) as ToolTask[];
}

/// Row-only `ToolTask`s for Bring-Your-Own tasks, so the Simulator has a row to render
/// (keyed by task name) while the diagnostic streams into it.
export async function buildMcpByoTasks(tasks: McpByoTaskDef[]): Promise<ToolTask[]> {
  return (await invoke("build_mcp_byo_tasks", { tasks })) as ToolTask[];
}

/// Run Bring-Your-Own diagnostics against the user's OWN servers, wired into the eval
/// eco-system: the backend emits the SAME batch events + persists a report keyed
/// `mcp:byo`, so the Simulator / Evaluator (live trace) / Model Results light up like a
/// Built-In run. DIAGNOSTIC only — schema-valid rate + attribution, no pass/fail verdict.
/// Cancellable via the shared Stop button. Resolves when the run completes.
export async function runMcpByoBatch(
  model: string,
  backend: BackendKind,
  tasks: McpByoTaskDef[],
): Promise<void> {
  await invoke("run_mcp_byo_batch", { model, backend, tasks });
}

import { invoke } from "@tauri-apps/api/core";
import type { BackendKind } from "../models/storage";
import type { McpTaskDef } from "../../../features/mcp/state/mcpStore";
import type { ToolTask } from "../eval/registry";

/// Convert built MCP tasks (world + oracle) into eval `ToolTask`s so they run
/// through the SAME batch pipeline as Built-In collections (Stage 2/3).
export async function buildMcpTasks(tasks: McpTaskDef[]): Promise<ToolTask[]> {
  return (await invoke("build_mcp_tasks", { tasks })) as ToolTask[];
}

/// Run a Bring-Your-Own diagnostic against the user's OWN server, wired into the
/// eval eco-system: the backend emits the SAME batch events + persists a report keyed
/// `mcp:byo`, so the Simulator / Evaluator (live trace) / Model Results light up like a
/// Built-In run. DIAGNOSTIC only — schema-valid rate + attribution, no pass/fail verdict.
/// Resolves when the run completes (progress arrives via the batch event stream).
export async function runMcpByoBatch(
  model: string,
  backend: BackendKind,
  serverId: string,
  taskName: string,
  instruction: string,
): Promise<void> {
  await invoke("run_mcp_byo_batch", { model, backend, serverId, taskName, instruction });
}

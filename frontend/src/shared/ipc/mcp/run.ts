import { invoke } from "@tauri-apps/api/core";
import type { BackendKind } from "../models/storage";
import type { McpTaskDef } from "../../../features/mcp/state/mcpStore";
import type { ToolTask } from "../eval/registry";

/// Convert built MCP tasks (world + oracle) into eval `ToolTask`s so they run
/// through the SAME batch pipeline as Built-In collections (Stage 2/3).
export async function buildMcpTasks(tasks: McpTaskDef[]): Promise<ToolTask[]> {
  return (await invoke("build_mcp_tasks", { tasks })) as ToolTask[];
}

/// Bring-Your-Own — one call graded for schema + fault attribution (no answer key).
export interface ByoCall {
  tool: string;
  schema_valid: boolean;
  attribution: "success" | "model" | "config" | "server";
  detail: string;
}
export interface ByoRunResult {
  total_calls: number;
  schema_valid: number;
  schema_valid_rate: number;
  model_faults: number;
  config_faults: number;
  server_faults: number;
  successes: number;
  calls: ByoCall[];
  assistant_text: string;
}

/// Run the model against the user's OWN server: a live trace + schema-valid rate +
/// whose-fault attribution. DIAGNOSTIC only — no world, no answer key, no verdict.
export async function runMcpByo(
  model: string,
  backend: BackendKind,
  serverId: string,
  instruction: string,
  maxSteps?: number,
): Promise<ByoRunResult> {
  return (await invoke("run_mcp_byo", { model, backend, serverId, instruction, maxSteps })) as ByoRunResult;
}

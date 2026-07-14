import { invoke } from "@tauri-apps/api/core";
import type { BackendKind } from "../models/storage";
import type { McpTaskDef } from "../../../features/mcp/state/mcpStore";

/// Track B — controlled-world pass^k verdict.
export interface McpRunResult {
  k: number;
  passes: number;
  ready: boolean;
  pass_rate: number;
  failures: string[][];
}

/// Track A — one call graded for schema + fault attribution.
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

/// Score a controlled-world task k times against a real model (graded on the
/// world end-state). The model + backend come from the global header selection.
export async function runMcpWorldTask(
  model: string,
  backend: BackendKind,
  task: McpTaskDef,
  maxSteps?: number,
): Promise<McpRunResult> {
  return (await invoke("run_mcp_world_task", { model, backend, task, maxSteps })) as McpRunResult;
}

/// Run the model once against the user's OWN server: schema-valid rate + whose-
/// fault attribution (no world, no answer key).
export async function runMcpByo(
  model: string,
  backend: BackendKind,
  serverId: string,
  instruction: string,
  maxSteps?: number,
): Promise<ByoRunResult> {
  return (await invoke("run_mcp_byo", { model, backend, serverId, instruction, maxSteps })) as ByoRunResult;
}

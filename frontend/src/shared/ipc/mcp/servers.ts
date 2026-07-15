import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

/// One configured MCP server. Mirrors `persistence::mcp::servers::McpServerConfig`.
/// Secret env VALUES are never here — only `env_keys` (their names); values live
/// in the OS keychain, set via `setMcpServerSecret`.
export const McpServerConfigSchema = z.object({
  id: z.string(),
  command: z.string(),
  args: z.array(z.string()).default([]),
  env_keys: z.array(z.string()).default([]),
  roots: z.array(z.string()).default([]),
  enabled: z.boolean().default(true),
});
export type McpServerConfig = z.infer<typeof McpServerConfigSchema>;

/// Result of connecting + listing tools — the "N tools discovered" preflight.
export const McpProbeSchema = z.object({
  server_name: z.string(),
  protocol_version: z.string(),
  tool_count: z.number().int().nonnegative(),
  tool_names: z.array(z.string()),
});
export type McpProbe = z.infer<typeof McpProbeSchema>;

export async function listMcpServers(): Promise<McpServerConfig[]> {
  return z.array(McpServerConfigSchema).parse(await invoke("list_mcp_servers"));
}

export async function upsertMcpServer(config: McpServerConfig): Promise<void> {
  await invoke("upsert_mcp_server", { config });
}

export async function removeMcpServer(id: string): Promise<void> {
  await invoke("remove_mcp_server", { id });
}

export async function setMcpServerEnabled(id: string, enabled: boolean): Promise<void> {
  await invoke("set_mcp_server_enabled", { id, enabled });
}

/// An app-managed scratch SQLite path for the sqlite quick-add, so the user needn't type an
/// absolute path (the server creates the file on first run).
export async function mcpScratchDbPath(): Promise<string> {
  return (await invoke("mcp_scratch_db_path")) as string;
}

/// Store a server env-var value in the OS keychain (never on disk).
export async function setMcpServerSecret(id: string, envVar: string, value: string): Promise<void> {
  await invoke("set_mcp_server_secret", { id, envVar, value });
}

/// Connect, list tools, disconnect. Throws with a loud diagnostic on failure.
export async function probeMcpServer(id: string): Promise<McpProbe> {
  return McpProbeSchema.parse(await invoke("probe_mcp_server", { id }));
}

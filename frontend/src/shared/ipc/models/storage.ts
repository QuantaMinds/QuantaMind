import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const BackendKindSchema = z.enum(["llama_cpp", "vllm"]);
export type BackendKind = z.infer<typeof BackendKindSchema>;

export const InstalledModelInfoSchema = z.object({
  name: z.string(),
  size_bytes: z.number().int().nonnegative(),
  modified_at: z.string(),
  family: z.string(),
  parameter_size: z.string(),
  quantization: z.string(),
  backend: BackendKindSchema,
  // Content hash of the model blob, when a backend reports one (shared across
  // tags of the same model); absent for llama.cpp. The picker dedupes on it.
  digest: z.string().optional(),
  // Friendly picker label when `name` isn't presentable (a backend may store its on-disk
  // path as `name` for wire-id matching and carries the HF repo here).
  display_name: z.string().optional(),
  // Absolute GGUF path — present for llama.cpp models, absent for remote backends.
  path: z.string().optional(),
});
export type InstalledModelInfo = z.infer<typeof InstalledModelInfoSchema>;

export const DiskUsageSchema = z.object({
  total_bytes: z.number().int().nonnegative(),
  free_bytes: z.number().int().nonnegative(),
  models_bytes: z.number().int().nonnegative(),
});
export type DiskUsage = z.infer<typeof DiskUsageSchema>;

/// Models a remote vLLM server currently serves (from its `/v1/models`). Empty
/// when the endpoint isn't configured or is unreachable — never an error, so it
/// composes into the installed-models `Promise.allSettled` fan-out.
export async function listVllmModels(): Promise<InstalledModelInfo[]> {
  return z.array(InstalledModelInfoSchema).parse(await invoke("list_vllm_models"));
}

export async function getDiskUsage(): Promise<DiskUsage> {
  const raw = await invoke("get_disk_usage");
  return DiskUsageSchema.parse(raw);
}

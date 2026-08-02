import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const UserSettingsSchema = z.object({
  theme: z.string().nullable().optional(),
  first_run_complete: z.boolean().default(false),
  community_prompt_shown: z.boolean().default(false),
  last_update_check_at: z.string().nullable().optional(),
  models_folder: z.string().nullable().optional(),
  // Remote vLLM OpenAI endpoint (a GPU box) + optional bearer key.
  vllm_url: z.string().nullable().optional(),
  vllm_api_key: z.string().nullable().optional(),
  /// Hourly accelerator price for the Test-run cost figures. NO default — absent
  /// means every dollar figure reads "n/a (no price basis)". A guessed price
  /// would understate a real bill, which is worse than showing none.
  gpu_hourly_usd: z.number().nullable().optional(),
  /// Fraction of that accelerator this app has (1.0 = the whole card).
  cost_utilization: z.number().nullable().optional(),
});
export type UserSettings = z.infer<typeof UserSettingsSchema>;

export async function getUserSettings(): Promise<UserSettings> {
  return UserSettingsSchema.parse(await invoke("get_user_settings"));
}

export async function setUserSettings(settings: UserSettings): Promise<void> {
  await invoke("set_user_settings", { settings });
}

/// Absolute path of the shared GGUF weights folder (setting → env → default).
export async function resolveModelsFolder(): Promise<string> {
  return z.string().parse(await invoke("resolve_models_folder"));
}

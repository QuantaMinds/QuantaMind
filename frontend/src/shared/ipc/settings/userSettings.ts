import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const UserSettingsSchema = z.object({
  theme: z.string().nullable().optional(),
  first_run_complete: z.boolean().default(false),
  last_update_check_at: z.string().nullable().optional(),
  models_folder: z.string().nullable().optional(),
  stt_engine_dir: z.string().nullable().optional(),
  // Remote vLLM/SGLang OpenAI endpoints (a GPU box) + optional bearer keys.
  vllm_url: z.string().nullable().optional(),
  vllm_api_key: z.string().nullable().optional(),
  sglang_url: z.string().nullable().optional(),
  sglang_api_key: z.string().nullable().optional(),
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

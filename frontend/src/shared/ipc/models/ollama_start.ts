import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const OllamaStartResultSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("already_running") }),
  z.object({ status: z.literal("started"), pid: z.number().int().nonnegative() }),
  z.object({ status: z.literal("not_installed"), install_url: z.string().url() }),
  z.object({ status: z.literal("start_failed"), error: z.string() }),
  z.object({ status: z.literal("manual_start_required"), install_url: z.string().url() }),
]);
export type OllamaStartResult = z.infer<typeof OllamaStartResultSchema>;

export async function startOllama(): Promise<OllamaStartResult> {
  const raw = await invoke("start_ollama");
  return OllamaStartResultSchema.parse(raw);
}

export async function stopOllama(): Promise<void> {
  await invoke("stop_ollama");
}

export async function isOllamaAutoStartSupported(): Promise<boolean> {
  return await invoke("ollama_auto_start_supported");
}

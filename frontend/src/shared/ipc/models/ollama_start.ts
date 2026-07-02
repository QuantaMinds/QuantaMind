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

/// Where Ollama placed a loaded model's weights (VRAM vs CPU). `on_cpu` true when it spilled
/// to system RAM (slower); `note` is a ready-to-show line. `null` when the model isn't loaded
/// or Ollama is unreachable — the UI shows nothing.
export const OllamaPlacementSchema = z
  .object({
    on_cpu: z.boolean(),
    total_bytes: z.number().nonnegative(),
    vram_bytes: z.number().nonnegative(),
    cpu_bytes: z.number().nonnegative(),
    note: z.string().nullable(),
  })
  .nullable();
export type OllamaPlacement = z.infer<typeof OllamaPlacementSchema>;

/// Probe how Ollama placed `model` (VRAM vs CPU) so the eval UI can warn that a CPU-offloaded
/// run is slower (and that the harness gives each step more time).
export async function ollamaModelPlacement(model: string): Promise<OllamaPlacement> {
  return OllamaPlacementSchema.parse(await invoke("ollama_model_placement", { model }));
}

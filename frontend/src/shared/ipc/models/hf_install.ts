import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const EVENT_HF_PROGRESS = "hf-progress";

/// The install is a single download into the shared weights folder, so
/// `downloading` is the only phase — there is no post-download import step.
export const HfPhaseSchema = z.object({
  phase: z.literal("downloading"),
  bytes_completed: z.number().int().nonnegative(),
  bytes_total: z.number().int().nonnegative(),
  speed_bps: z.number().int().nonnegative(),
});
export type HfPhase = z.infer<typeof HfPhaseSchema>;

export async function installHfGguf(
  repo: string,
  filename: string,
  name: string,
): Promise<void> {
  await invoke("install_hf_gguf", { repo, filename, name });
}

export async function cancelHfInstall(): Promise<void> {
  await invoke("cancel_hf_install");
}

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export const RECOMMENDED_MODEL = "Llama 3.2 1B Instruct (GGUF)";

/// Create ~/Documents/QuantaMind with a welcome prompt; returns the root.
export async function scaffoldOnboardingWorkspace(): Promise<string> {
  return z.string().parse(await invoke("scaffold_onboarding_workspace"));
}

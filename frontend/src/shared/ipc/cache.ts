import { invoke } from "@tauri-apps/api/core";

/// Clear regenerable app caches (eval history, batch reports, job logs, traces,
/// context-cliff measurements, recent-workspace list). Custom eval collections,
/// readiness profiles, and user settings are never touched. When `includeModels`
/// is set, also wipes the re-downloadable HuggingFace snapshot cache (MLX
/// weights); the app's canonical `~/.quantamind` models are still left intact.
/// Returns the number of bytes freed.
export async function clearAppCache(includeModels: boolean): Promise<number> {
  return (await invoke("clear_app_cache", { includeModels })) as number;
}

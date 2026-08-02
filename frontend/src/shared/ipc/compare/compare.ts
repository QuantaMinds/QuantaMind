import { invoke } from "@tauri-apps/api/core";
import type { InferenceParams } from "../workspace/prompts";
import type { BackendKind } from "../models/storage";

export type CompareStrategy = "sequential" | "parallel" | "sequential_skippable";

export interface RunCompareArgs {
  models: string[];
  prompt: string;
  strategy: CompareStrategy;
  system?: string;
  params?: InferenceParams;
  perModelParams?: Record<string, InferenceParams>;
  // One backend per model (parallel to `models`); each model runs on its own.
  backends?: BackendKind[];
  // the server keep_alive: -1 keep loaded, 0 unload after each model.
  keepAlive?: number;
}

export async function runCompare(args: RunCompareArgs): Promise<void> {
  await invoke("run_compare", { ...args });
}

export async function stopCompare(modelId?: string): Promise<void> {
  await invoke("stop_compare", { modelId: modelId ?? null });
}

export type CompareReportFormat = "md" | "json" | "html";

export async function saveCompareReport(
  path: string,
  format: CompareReportFormat,
  contents: string,
): Promise<void> {
  await invoke("save_compare_report", { path, format, contents });
}

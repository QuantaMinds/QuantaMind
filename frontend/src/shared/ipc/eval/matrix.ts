import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { BackendKindSchema, type BackendKind } from "../models/storage";
import { ToolCallReportSchema } from "./toolcall";
import type { ToolTask } from "./registry";

/// One model+backend to run a collection against.
export interface ModelTarget {
  model: string;
  backend: BackendKind;
  /// Reasoning model (sidebar "thinking" toggle): the backend raises its per-turn token
  /// budget and strips <think> before scoring. Optional — omitted/false = a terse model.
  is_thinking?: boolean;
}

/// One model's outcome for the whole collection — a report, or the error it hit.
export const MatrixColumnSchema = z.object({
  model: z.string(),
  backend: BackendKindSchema,
  report: ToolCallReportSchema.nullable(),
  error: z.string().nullable(),
});
export type MatrixColumn = z.infer<typeof MatrixColumnSchema>;

export const MatrixReportSchema = z.object({
  collection_id: z.string(),
  columns: z.array(MatrixColumnSchema),
  avg_score: z.number().nullable(),
});
export type MatrixReport = z.infer<typeof MatrixReportSchema>;

/// One recorded run for the regression timeline.
export const RunSummarySchema = z.object({
  ts: z.string(),
  model: z.string(),
  backend: BackendKindSchema,
  parse_rate: z.number().nullable(),
  tool_selection_acc: z.number().nullable(),
  arg_acc: z.number().nullable(),
  abstain_acc: z.number().nullable(),
  composite: z.number().nullable(),
  n: z.number().int().nonnegative(),
  // Agentic metrics (Phase 6) — absent in pre-Phase-6 history, so optional.
  pass_k: z.number().nullable().optional(),
  agentic_avg_steps: z.number().nullable().optional(),
  effort: z.number().nullable().optional(),
});
export type RunSummary = z.infer<typeof RunSummarySchema>;

/// Batch-run a collection's tasks against several models (sequential, backend
/// side) and get a per-model matrix; successful columns are logged to history.
export async function runCollectionMatrix(
  collectionId: string,
  targets: ModelTarget[],
  tasks: ToolTask[],
): Promise<MatrixReport> {
  return MatrixReportSchema.parse(
    await invoke("run_collection_matrix", { collectionId, targets, tasks }),
  );
}

export const LoadedHistorySchema = z.object({
  entries: z.array(RunSummarySchema),
  /// Stored rows this build couldn't interpret (e.g. recorded against a backend
  /// that no longer exists). Skipped rather than failing the whole read, and
  /// counted so the UI can say so instead of quietly showing a short list.
  unreadable: z.number().int().nonnegative(),
});
export type LoadedHistory = z.infer<typeof LoadedHistorySchema>;

/// The recorded run history for a collection, oldest first.
export async function loadCollectionHistory(collectionId: string): Promise<LoadedHistory> {
  return LoadedHistorySchema.parse(await invoke("load_collection_history", { collectionId }));
}

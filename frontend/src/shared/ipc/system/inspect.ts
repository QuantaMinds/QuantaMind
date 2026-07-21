import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { BackendKind } from "../models/storage";

/// Model metadata from Ollama's /api/show (template, capabilities) + an advisory
/// base-model guess. `available` is false on non-Ollama backends.
export const ModelDimsSchema = z.object({
  layers: z.number().int().nonnegative(),
  head_count: z.number().int().nonnegative(),
  head_count_kv: z.number().int().nonnegative(),
  embedding_length: z.number().int().nonnegative(),
  context_length: z.number().int().nonnegative(),
  // head_count_kv was defaulted to head_count (the model didn't report it — e.g.
  // qwen35): the KV-cache figure is then a conservative OVER-estimate. Surfaced as
  // "~" so a ceiling built on it never reads as exact. Optional for back-compat.
  kv_estimated: z.boolean().optional(),
});
export type ModelDims = z.infer<typeof ModelDimsSchema>;

export const ModelInspectSchema = z.object({
  available: z.boolean(),
  note: z.string().nullable(),
  template: z.string(),
  capabilities: z.array(z.string()),
  family: z.string().nullable(),
  parameter_size: z.string().nullable(),
  quantization: z.string().nullable(),
  is_base_guess: z.boolean(),
  base_reason: z.string().nullable(),
  dims: ModelDimsSchema.nullable(),
});
export type ModelInspect = z.infer<typeof ModelInspectSchema>;

export async function inspectModel(model: string, backend: BackendKind): Promise<ModelInspect> {
  return ModelInspectSchema.parse(await invoke("inspect_model", { model, backend }));
}

/// KV-cache storage precision (llama.cpp/Ollama cache-type wire names). f16 is
/// the conservative baseline; q8_0 ≈ half the bytes, q4_0 ≈ a quarter — exact
/// integer scaling of the canonical formula, computed in Rust.
export type KvPrecision = "f16" | "q8_0" | "q4_0";

/// KV-cache size (bytes) for a model's dims at a context length — computed by
/// the canonical Rust formula (single source of truth). `precision` omitted →
/// f16, byte-identical to every pre-existing call.
export async function estimateKvCacheBytes(dims: ModelDims, contextLength: number, precision?: KvPrecision): Promise<number> {
  return z.number().parse(
    await invoke("estimate_kv_cache_bytes", {
      layers: dims.layers,
      headCount: dims.head_count,
      headCountKv: dims.head_count_kv,
      embeddingLength: dims.embedding_length,
      contextLength,
      precision: precision ?? null,
    }),
  );
}

/// Whether the WEIGHTS fit under the GPU's hard memory limit — the question the
/// context ceilings alone can't answer (a 100K ceiling is meaningless if the model
/// doesn't even load on the GPU). `spills_to_cpu` = weights exceed the limit → CPU/swap,
/// very slow. `unknown` = the limit is unmeasured (off Apple Silicon), never guessed.
export const FitVerdictSchema = z.enum(["fits", "tight", "spills_to_cpu", "unknown"]);
export type FitVerdict = z.infer<typeof FitVerdictSchema>;

/// The largest context this machine holds for a model at each KV-cache precision, plus
/// the weights' fit verdict against the GPU limit. A `null` ceiling means unmeasurable
/// ("Not available"), never a guess. On Apple Silicon the ceilings budget against the
/// MEASURED Metal working-set limit (`workingSetBytes`), so "fits" means fits on the GPU.
/// `fit` is optional for back-compat (older payloads / fixtures omit it → treated unknown).
export const CtxCeilingsSchema = z.object({
  f16: z.number().int().nullable(),
  q8: z.number().int().nullable(),
  q4: z.number().int().nullable(),
  fit: FitVerdictSchema.optional(),
});
export type CtxCeilings = z.infer<typeof CtxCeilingsSchema>;

export async function contextCeilings(
  dims: ModelDims,
  modelBytes: number,
  totalBytes: number,
  workingSetBytes?: number | null,
): Promise<CtxCeilings> {
  return CtxCeilingsSchema.parse(
    await invoke("context_ceilings", {
      layers: dims.layers,
      headCount: dims.head_count,
      headCountKv: dims.head_count_kv,
      embeddingLength: dims.embedding_length,
      modelBytes,
      totalBytes,
      workingSetBytes: workingSetBytes ?? null,
    }),
  );
}

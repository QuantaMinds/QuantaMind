import type { BackendKind } from "../ipc/models/storage";

/// Only Ollama serves arbitrary models by name. llama.cpp (`llama-server`) and
/// MLX (`mlx_lm.server`) are single-model servers — they serve the one model
/// loaded at launch and ignore the requested name — so multi-model features
/// (quant comparison, evals across models) can only switch models on Ollama.
export function servesModelsByName(backend: BackendKind): boolean {
  return backend === "ollama";
}

export const SINGLE_MODEL_NOTE =
  "llama.cpp & MLX serve one model at a time — this runs whichever model the server has loaded. Load this exact model, or use Ollama to compare across models.";

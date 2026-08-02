import type { BackendKind } from "../../../shared/ipc/models/storage";

export interface BackendHealth {
  llama: boolean | null;
  vllm: boolean | null;
  sglang: boolean | null;
}

/// The blocking hint shown on the Run button when the active backend (= the
/// selected model's required backend) isn't healthy. A backend is coupled to the
/// model's weight format, so there is NO fallback — the user must start the
/// right server. Returns null when the run isn't blocked by backend health.
export function backendRunHint(backend: BackendKind, health: BackendHealth): string | null {
  if (backend === "llama_cpp") return health.llama === true ? null : "Start llama.cpp to run this model";
  if (backend === "vllm")
    return health.vllm === true ? null : "Set the vLLM server URL in Settings and start it";
  if (backend === "sglang")
    return health.sglang === true ? null : "Set the SGLang server URL in Settings and start it";
  return null;
}

import type { BackendKind } from "../../../../shared/ipc/models/storage";

export type BackendStatus = { running: boolean; label: string; aria: string };

/// Status-bar dot + text for the active backend. llama.cpp tracks its local
/// server's run state; vLLM is remote so it reads as connected / not
/// reachable. Each names the loaded model.
export function backendStatus(
  backend: BackendKind,
  llamaHealthy: boolean | null,
  vllmHealthy: boolean | null,
  model: string | null,
): BackendStatus {
  const named = model ? ` (${model})` : "";
  if (backend === "vllm") {
    const running = vllmHealthy === true;
    return {
      running,
      aria: "vLLM health",
      label: running ? `vLLM · connected${named}` : "vLLM · not reachable",
    };
  }
  const running = llamaHealthy === true;
  return {
    running,
    aria: "llama.cpp health",
    label: running ? `llama.cpp · running${named}` : "llama.cpp · not started",
  };
}

import type { HealthStatus } from "../../../../shared/ipc/core/types";
import type { BackendKind } from "../../../../shared/ipc/models/storage";

export type BackendStatus = { running: boolean; label: string; aria: string };

/// Status-bar dot + text for the active backend. Ollama uses its polled health
/// (naming the version when connected); llama.cpp and MLX track their local
/// server's run state; vLLM/SGLang are remote so they read as connected/not
/// reachable. Each names the loaded model. Without this, the others would fall
/// through to the Ollama label ("Ollama not running") regardless of backend.
export function backendStatus(
  backend: BackendKind,
  health: HealthStatus | null,
  llamaHealthy: boolean | null,
  mlxHealthy: boolean | null,
  vllmHealthy: boolean | null,
  sglangHealthy: boolean | null,
  model: string | null,
): BackendStatus {
  const named = model ? ` (${model})` : "";
  if (backend === "llama_cpp") {
    const running = llamaHealthy === true;
    return {
      running,
      aria: "llama.cpp health",
      label: running ? `llama.cpp · running${named}` : "llama.cpp · not started",
    };
  }
  if (backend === "mlx") {
    const running = mlxHealthy === true;
    return {
      running,
      aria: "MLX health",
      label: running ? `MLX · running${named}` : "MLX · not running",
    };
  }
  if (backend === "vllm") {
    const running = vllmHealthy === true;
    return {
      running,
      aria: "vLLM health",
      label: running ? `vLLM · connected${named}` : "vLLM · not reachable",
    };
  }
  if (backend === "sglang") {
    const running = sglangHealthy === true;
    return {
      running,
      aria: "SGLang health",
      label: running ? `SGLang · connected${named}` : "SGLang · not reachable",
    };
  }
  const running = health?.available === true;
  const label =
    health === null
      ? "checking…"
      : running
        ? `connected${health.version ? ` · ${health.version}` : ""}`
        : "Ollama not running";
  return { running, aria: "Ollama health", label };
}

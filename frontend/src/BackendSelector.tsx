import type { BackendKind } from "./shared/ipc/models/storage";
import { useBackendStore } from "./shared/state/backendStore";
import { useMlxBackend } from "./features/workspace/hooks/useMlxBackend";
import { useLlamaBackend } from "./features/workspace/hooks/useLlamaBackend";
import { useVllmBackend, useSglangBackend } from "./features/workspace/hooks/useRemoteBackends";

const BASE_BACKENDS: { id: BackendKind; label: string }[] = [
  { id: "llama_cpp", label: "llama.cpp" },
  { id: "ollama", label: "Ollama" },
  { id: "vllm", label: "vLLM" },
  { id: "sglang", label: "SGLang" },
];

function dotClass(healthy: boolean | null): string {
  const color = healthy === null ? "bg-gray-300" : healthy ? "bg-green-500" : "bg-gray-400";
  return `inline-block h-2 w-2 rounded-full ${color}`;
}

/// The global LLM-backend picker in the header — a dropdown (llama.cpp / Ollama,
/// plus MLX on Apple Silicon where mlx_lm.server can run). The whole app scopes
/// its model list and runs to the selected backend (architecture.md rule 7). The
/// dot reflects the selected backend's server: green = running. useMlxBackend /
/// useLlamaBackend poll their health into backendStore (Ollama via the StatusBar).
export function BackendSelector() {
  const { appleSilicon } = useMlxBackend();
  useLlamaBackend();
  useVllmBackend();
  useSglangBackend();
  const selected = useBackendStore((s) => s.selectedBackend);
  const setSelected = useBackendStore((s) => s.setSelectedBackend);
  const healthy = useBackendStore((s) => s.isHealthy(selected));
  const backends = appleSilicon
    ? [...BASE_BACKENDS, { id: "mlx" as BackendKind, label: "MLX" }]
    : BASE_BACKENDS;
  return (
    <div
      data-testid="header-backend-selector"
      className="flex items-center gap-1.5 border rounded px-2 py-1"
      title={healthy ? "server running" : "server stopped"}
    >
      <span className={dotClass(healthy)} aria-hidden />
      <select
        data-testid="header-backend-select"
        aria-label="LLM backend"
        value={selected}
        onChange={(e) => setSelected(e.target.value as BackendKind)}
        className="text-sm bg-transparent outline-none cursor-pointer"
      >
        {backends.map((b) => (
          <option key={b.id} value={b.id}>
            {b.label}
          </option>
        ))}
      </select>
    </div>
  );
}

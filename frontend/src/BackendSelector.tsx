import { useEffect } from "react";
import type { BackendKind } from "./shared/ipc/models/storage";
import { useBackendStore } from "./shared/state/backendStore";
import { useLlamaBackend, useVllmBackend } from "./features/workspace/hooks/useBackendHealth";
import { useRemoteEndpointsStore } from "./features/workspace/state/remoteEndpointsStore";

const BACKENDS: { id: BackendKind; label: string }[] = [
  { id: "llama_cpp", label: "llama.cpp" },
  { id: "vllm", label: "vLLM" },
];

function dotClass(healthy: boolean | null): string {
  const color = healthy === null ? "bg-gray-300" : healthy ? "bg-green-500" : "bg-gray-400";
  return `inline-block h-2 w-2 rounded-full ${color}`;
}

/// The global LLM-backend picker in the header — a dropdown (llama.cpp, plus the
/// remote vLLM server). The whole app scopes its model list and runs to
/// the selected backend (architecture.md rule 7). The dot reflects the selected
/// backend's server: green = running. The `use*Backend` hooks poll health into
/// backendStore.
export function BackendSelector() {
  // Load the configured remote endpoint once so the vLLM poller can gate on it —
  // without this they'd default to unconfigured and never poll even when an endpoint IS set.
  useEffect(() => {
    void useRemoteEndpointsStore.getState().load();
  }, []);
  useLlamaBackend();
  useVllmBackend();
  const selected = useBackendStore((s) => s.selectedBackend);
  const setSelected = useBackendStore((s) => s.setSelectedBackend);
  const healthy = useBackendStore((s) => s.isHealthy(selected));
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
        {BACKENDS.map((b) => (
          <option key={b.id} value={b.id}>
            {b.label}
          </option>
        ))}
      </select>
    </div>
  );
}

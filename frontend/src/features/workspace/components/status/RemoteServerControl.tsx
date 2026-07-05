import type { BackendKind } from "../../../../shared/ipc/models/storage";
import { useBackendStore } from "../../../../shared/state/backendStore";

const LABELS: Record<"vllm" | "sglang", string> = { vllm: "vLLM", sglang: "SGLang" };

/// Read-only header status for a remote OpenAI backend (vLLM / SGLang). Unlike
/// the local sidecars the app can't start/stop these — they run on a remote GPU —
/// so this shows only a health dot + a hint to configure the endpoint in Settings.
/// The dot follows the polled health from `useVllmBackend`/`useSglangBackend`.
export function RemoteServerControl({ backend }: { backend: "vllm" | "sglang" }) {
  const healthy = useBackendStore((s) => (backend === "vllm" ? s.vllmHealthy : s.sglangHealthy));
  const label = LABELS[backend];
  const color = healthy === null ? "bg-gray-300" : healthy ? "bg-green-500" : "bg-gray-400";
  const title = healthy
    ? `${label} server reachable`
    : `Set the ${label} server URL in Settings, then start it on your GPU`;
  return (
    <div
      className="flex items-center gap-1.5 px-2 text-xs"
      title={title}
      data-testid={`${backend}-status`}
    >
      <span className={`inline-block h-2 w-2 rounded-full ${color}`} aria-hidden />
      <span>{healthy ? `${label} · connected` : `${label} · configure in Settings`}</span>
    </div>
  );
}

// Narrow the app-wide BackendKind to the two remote backends this control serves.
export function isRemoteBackend(b: BackendKind): b is "vllm" | "sglang" {
  return b === "vllm" || b === "sglang";
}

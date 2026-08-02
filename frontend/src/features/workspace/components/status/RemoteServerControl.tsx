import type { BackendKind } from "../../../../shared/ipc/models/storage";
import { useBackendStore } from "../../../../shared/state/backendStore";

/// Read-only header status for the remote OpenAI backend (vLLM). Unlike the local
/// sidecar the app can't start/stop it — it runs on a remote GPU — so this shows
/// only a health dot + a hint to configure the endpoint in Settings. The dot
/// follows the polled health from `useVllmBackend`.
export function RemoteServerControl({ backend }: { backend: "vllm" }) {
  const healthy = useBackendStore((s) => s.vllmHealthy);
  const label = "vLLM";
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

// Narrow the app-wide BackendKind to the remote backend this control serves.
export function isRemoteBackend(b: BackendKind): b is "vllm" {
  return b === "vllm";
}

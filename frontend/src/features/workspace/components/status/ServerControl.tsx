import { useBackendStore } from "../../../../shared/state/backendStore";
import { LlamaServerControl } from "./LlamaServerControl";
import { RemoteServerControl, isRemoteBackend } from "./RemoteServerControl";

/// The single header Start/Stop control. Reflects the active backend chosen in
/// the BackendPanel and starts/stops that server (not the prompt run). Remote
/// backend (vLLM) can't be started by the app, so it gets a read-only
/// status readout instead.
export function ServerControl() {
  const activeBackend = useBackendStore((s) => s.selectedBackend);
  if (isRemoteBackend(activeBackend)) return <RemoteServerControl backend={activeBackend} />;
  return <LlamaServerControl />;
}

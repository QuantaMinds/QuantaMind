import { useBackendStore } from "../../../../shared/state/backendStore";
import { OllamaControl } from "./OllamaControl";
import { LlamaServerControl } from "./LlamaServerControl";
import { MlxServerControl } from "./MlxServerControl";
import { RemoteServerControl, isRemoteBackend } from "./RemoteServerControl";

/// The single header Start/Stop control. Reflects the active backend chosen in
/// the BackendPanel and starts/stops that server (not the prompt run). Remote
/// backends (vLLM/SGLang) can't be started by the app, so they get a read-only
/// status readout instead.
export function ServerControl() {
  const activeBackend = useBackendStore((s) => s.selectedBackend);
  if (activeBackend === "ollama") return <OllamaControl />;
  if (activeBackend === "mlx") return <MlxServerControl />;
  if (isRemoteBackend(activeBackend)) return <RemoteServerControl backend={activeBackend} />;
  return <LlamaServerControl />;
}

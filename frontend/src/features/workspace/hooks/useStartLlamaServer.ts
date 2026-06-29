import { useCallback, useState } from "react";
import { startLlamaServer } from "../../../shared/ipc/models/llama_start";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { useBackendStore } from "../../../shared/state/backendStore";

export type StartLlamaStatus =
  | "idle" | "starting" | "success" | "error" | "not_bundled";

/// Start the llama-server sidecar on a specific GGUF path (one at a time).
export function useStartLlamaServer() {
  const [status, setStatus] = useState<StartLlamaStatus>("idle");
  const [error, setError] = useState<string | null>(null);

  const start = useCallback(async (modelPath: string, numCtx?: number | null) => {
    setError(null);
    setStatus("starting");
    try {
      const result = await startLlamaServer(modelPath, numCtx);
      switch (result.status) {
        case "already_running":
        case "started":
          useBackendStore.getState().setLlamaHealthy(true);
          setStatus("idle");
          return;
        case "not_bundled":
          setError(result.note);
          setStatus("not_bundled");
          return;
        case "start_failed":
          setError(result.error);
          setStatus("error");
          return;
      }
    } catch (e) {
      setError(formatIpcError(e));
      setStatus("error");
    }
  }, []);

  return { status, error, start };
}

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
  // A benign, user-facing note when the backend applied a hardware constraint at launch
  // (flash attention / Q8 KV cache / capped context on a tight host) — distinct from `error`:
  // the server DID start, this just explains how it's running safely.
  const [notice, setNotice] = useState<string | null>(null);

  const start = useCallback(async (modelPath: string, numCtx?: number | null) => {
    setError(null);
    setNotice(null);
    setStatus("starting");
    try {
      const result = await startLlamaServer(modelPath, numCtx);
      switch (result.status) {
        case "started":
        case "already_running":
          // A fresh start may carry a hardware-constraint note; an already-running server can't.
          if (result.status === "started" && result.note) setNotice(result.note);
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

  return { status, error, notice, start };
}

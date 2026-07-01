import { useCallback, useEffect, useState } from "react";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import {
  isOllamaAutoStartSupported,
  startOllama,
} from "../../../shared/ipc/models/ollama_start";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBackendStore } from "../../../shared/state/backendStore";

export type StartOllamaStatus =
  | "idle" | "starting" | "success" | "error" | "not_installed" | "manual_start_required";

const SUCCESS_LINGER_MS = 1000;

export function useStartOllama() {
  const [status, setStatus] = useState<StartOllamaStatus>("idle");
  const [error, setError] = useState<string | null>(null);
  const [installUrl, setInstallUrl] = useState<string | null>(null);
  // Starts true so the idle view offers "Start Ollama" while the one-time OS
  // check resolves — flips to false only once the backend confirms this OS
  // can't auto-launch, never the other way, so there's no button flicker.
  const [autoStartSupported, setAutoStartSupported] = useState(true);

  useEffect(() => {
    isOllamaAutoStartSupported()
      .then(setAutoStartSupported)
      .catch(() => {});
  }, []);

  const onSuccess = useCallback(() => {
    setStatus("success");
    setTimeout(() => {
      useBackendStore.getState().setOllamaHealthy(true);
      void useInstalledModelsStore.getState().refresh();
      setStatus("idle");
    }, SUCCESS_LINGER_MS);
  }, []);

  const start = useCallback(async () => {
    setError(null);
    setInstallUrl(null);
    setStatus("starting");
    try {
      const result = await startOllama();
      switch (result.status) {
        case "already_running":
        case "started":
          onSuccess();
          return;
        case "not_installed":
          setInstallUrl(result.install_url);
          setStatus("not_installed");
          return;
        case "manual_start_required":
          setInstallUrl(result.install_url);
          setStatus("manual_start_required");
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
  }, [onSuccess]);

  const openInstallPage = useCallback(async () => {
    const url = installUrl ?? "https://ollama.com/download";
    try { await openExternal(url); }
    catch (e) { setError(formatIpcError(e)); setStatus("error"); }
  }, [installUrl]);

  return { status, error, installUrl, autoStartSupported, start, openInstallPage };
}

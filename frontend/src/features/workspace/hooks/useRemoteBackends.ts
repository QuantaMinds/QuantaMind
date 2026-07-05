import { useEffect } from "react";
import { checkVllmHealth, checkSglangHealth } from "../../../shared/ipc/core/client";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";

const POLL_MS = 5000;

// Polls a remote OpenAI backend's health into the store every 5s — matching how
// llama.cpp/MLX are polled. Health is false until the endpoint is configured
// (Settings) and reachable; vLLM/SGLang run on a remote GPU so there's no
// platform gate. On the false/null→true edge (server just became reachable) it
// refreshes the installed-models list, since the remote model list is served by
// the endpoint (`/v1/models`) and is otherwise only fetched at startup / on
// install events — so a model configured after startup would never appear.
function useRemoteHealth(probe: () => Promise<{ available: boolean }>, set: (h: boolean) => void): void {
  useEffect(() => {
    let cancelled = false;
    let prev: boolean | null = null;
    const tick = async () => {
      let available = false;
      try {
        available = (await probe()).available;
      } catch {
        available = false;
      }
      if (cancelled) return;
      set(available);
      if (available && prev !== true) {
        void useInstalledModelsStore.getState().refresh();
      }
      prev = available;
    };
    void tick();
    const id = setInterval(() => void tick(), POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [probe, set]);
}

export function useVllmBackend(): void {
  useRemoteHealth(checkVllmHealth, useBackendStore((s) => s.setVllmHealthy));
}

export function useSglangBackend(): void {
  useRemoteHealth(checkSglangHealth, useBackendStore((s) => s.setSglangHealthy));
}

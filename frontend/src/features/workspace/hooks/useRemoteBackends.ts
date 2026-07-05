import { useEffect } from "react";
import { checkVllmHealth, checkSglangHealth } from "../../../shared/ipc/core/client";
import { useBackendStore } from "../../../shared/state/backendStore";

const POLL_MS = 5000;

// Polls a remote OpenAI backend's health into the store every 5s — matching how
// llama.cpp/MLX are polled. Health is false until the endpoint is configured
// (Settings) and reachable; vLLM/SGLang run on a remote GPU so there's no
// platform gate.
function useRemoteHealth(probe: () => Promise<{ available: boolean }>, set: (h: boolean) => void): void {
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const h = await probe();
        if (!cancelled) set(h.available);
      } catch {
        if (!cancelled) set(false);
      }
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

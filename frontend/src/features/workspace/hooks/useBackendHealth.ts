import { useEffect, useRef } from "react";
import type { BackendKind } from "../../../shared/ipc/models/storage";
import { checkLlamaHealth, checkVllmHealth } from "../../../shared/ipc/core/client";
import { useBackendStore } from "../../../shared/state/backendStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useRemoteEndpointsStore } from "../state/remoteEndpointsStore";

// The whole app is scoped to ONE backend at a time, so only that backend needs live health.
const FAST_MS = 5000; // the SELECTED backend — kept live for the picker dot + run pre-flight
const SLOW_MS = 30000; // background backends — a slow heartbeat, just so the picker dot isn't stale

/// Polls one backend's health into the store, at a cadence that depends on whether it is the
/// SELECTED backend (5s) or a background one (30s), and only while `enabled`. Centralizes the
/// selected-aware interval + enable gate so a backend the user isn't running isn't probed every
/// 5s — the old code probed every backend unconditionally (including remote vLLM with no
/// endpoint configured). `onBecameAvailable` fires on the false/null→true edge (e.g. refresh a
/// remote model list when its server first comes up).
export function usePolledBackendHealth(
  backend: BackendKind,
  probe: () => Promise<{ available: boolean }>,
  set: (h: boolean) => void,
  opts?: { enabled?: boolean; onBecameAvailable?: () => void },
): void {
  const isSelected = useBackendStore((s) => s.selectedBackend === backend);
  const enabled = opts?.enabled ?? true;
  // Held in a ref so a fresh inline callback each render doesn't re-establish the interval.
  const onEdge = useRef(opts?.onBecameAvailable);
  onEdge.current = opts?.onBecameAvailable;

  useEffect(() => {
    if (!enabled) {
      // Not runnable (remote endpoint unconfigured, or platform-gated) → definitively down,
      // and NOT polled at all.
      set(false);
      return;
    }
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
      if (available && prev !== true) onEdge.current?.();
      prev = available;
    };
    void tick();
    const id = setInterval(() => void tick(), isSelected ? FAST_MS : SLOW_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [backend, enabled, isSelected, probe, set]);
}

// Polls the llama.cpp sidecar's health — fast while selected, slow heartbeat otherwise. Without a
// re-probe, llamaHealthy was only set true on start and never refreshed, so it went stale (still
// "healthy") after the server died. llama.cpp runs on any platform, so no gate.
export function useLlamaBackend(): void {
  const setLlamaHealthy = useBackendStore((s) => s.setLlamaHealthy);
  usePolledBackendHealth("llama_cpp", checkLlamaHealth, setLlamaHealthy);
}

// vLLM runs on a remote GPU box. It is polled ONLY when an endpoint is configured in
// Settings — an unconfigured remote backend is never probed (the old code hit a blank endpoint
// every 5s → constant connection-refused). On the unreachable→reachable edge the installed-models
// list is refreshed, since the remote model list is served by the endpoint (`/v1/models`).
const refreshModels = () => void useInstalledModelsStore.getState().refresh();

export function useVllmBackend(): void {
  const set = useBackendStore((s) => s.setVllmHealthy);
  const configured = useRemoteEndpointsStore((s) => s.vllmUrl != null);
  usePolledBackendHealth("vllm", checkVllmHealth, set, { enabled: configured, onBecameAvailable: refreshModels });
}

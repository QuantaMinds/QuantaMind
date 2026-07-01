import { useEffect, useState } from "react";
import { osPlatform, type HostOs } from "../ipc/system/os_platform";

/// The host OS as detected by the backend — memoized in module scope so the
/// invoke happens exactly once per app session (mirroring how
/// `shortcuts.ts::isMac` reads `navigator.platform` once). Components that
/// need the OS for per-OS setup copy call this hook.
let cached: HostOs | null = null;
let inFlight: Promise<HostOs> | null = null;

async function loadOnce(): Promise<HostOs> {
  if (cached !== null) return cached;
  if (inFlight !== null) return inFlight;
  inFlight = osPlatform().then((os: HostOs) => {
    cached = os;
    inFlight = null;
    return os;
  });
  return inFlight;
}

/// Returns the host OS, or `null` while the initial IPC is in flight.
/// Consumers that need copy immediately (setup cards on first paint) should
/// tolerate `null` by falling back to a generic default.
export function useHostOs(): HostOs | null {
  const [os, setOs] = useState<HostOs | null>(cached);
  useEffect(() => {
    if (os !== null) return;
    let alive = true;
    loadOnce().then((v) => {
      if (alive) setOs(v);
    });
    return () => {
      alive = false;
    };
  }, [os]);
  return os;
}

/// Test-only escape hatch — resets the module cache so a test can re-mount
/// with a fresh probe. Not exported from the public barrel.
export function __resetHostOsCacheForTesting(): void {
  cached = null;
  inFlight = null;
}

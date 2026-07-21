import { useEffect, useState } from "react";
import { getHardwareSnapshot, type HardwareSnapshot } from "../../../shared/ipc/compare/hardware";

/// Fetch the hardware snapshot once so the Inspector can scale the VRAM bar
/// against the device's memory pool. Errors degrade to null.
export function useHardware(): HardwareSnapshot | null {
  const [hw, setHw] = useState<HardwareSnapshot | null>(null);
  useEffect(() => {
    getHardwareSnapshot().then(setHw).catch(() => setHw(null));
  }, []);
  return hw;
}

/// Device memory pool total, whether it's unified, and — on Apple Silicon — the GPU's
/// MEASURED Metal working-set limit (the real budget for weights + KV cache, below the
/// total pool). Derived from a snapshot: unified (Apple) → system RAM + working set;
/// discrete (NVIDIA) → VRAM total (VRAM already IS the budget, so no separate working
/// set); else null.
export function deviceMemory(hw: HardwareSnapshot | null): {
  totalBytes: number | null;
  unified: boolean;
  workingSetBytes: number | null;
} {
  const g = hw?.gpu;
  if (g?.unified)
    return { totalBytes: hw?.total_memory_bytes ?? null, unified: true, workingSetBytes: g?.gpu_working_set_bytes ?? null };
  return { totalBytes: g?.vram_total_bytes ?? null, unified: false, workingSetBytes: null };
}

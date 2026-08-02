import type { LoadedModel } from "../../../shared/ipc/system/vram";

export interface VramUsage {
  usedBytes: number; // resident in the device pool (VRAM / unified memory)
  totalBytes: number; // device memory total (falls back to the model size)
  offloadBytes: number; // footprint spilled to system RAM (discrete GPUs)
  pct: number; // usedBytes / totalBytes, clamped to 100
}

/// A model's memory footprint as a fraction of the device's memory pool.
/// `deviceTotalBytes` = unified RAM (Apple) or VRAM total (NVIDIA); when
/// unknown, falls back to the model size so the bar still renders. `offload`
/// is the part spilled to system RAM (size − resident). Pure.
///
/// `unified` (Apple Silicon): the CPU and GPU share ONE memory pool, so a loaded model is
/// fully resident "in unified memory" and NOTHING is offloaded — the VRAM/RAM split (and the
/// `size_vram` field, however each backend reports it) is meaningless here. So on unified the
/// whole footprint counts as resident and `offload` is 0, for every backend.
export function vramUsage(
  sizeBytes: number,
  sizeVramBytes: number,
  deviceTotalBytes?: number | null,
  unified = false,
): VramUsage {
  const size = Math.max(0, sizeBytes);
  const usedBytes = unified ? size : Math.max(0, Math.min(sizeVramBytes, size));
  const offloadBytes = Math.max(0, size - usedBytes);
  const totalBytes = deviceTotalBytes && deviceTotalBytes > 0 ? deviceTotalBytes : size || 1;
  const pct = Math.min(100, (usedBytes / totalBytes) * 100);
  return { usedBytes, totalBytes, offloadBytes, pct };
}

/// Look up a run's model in /api/ps results, tolerating the `:latest` tag form
/// (a run may use "phi3.5" while /api/ps reports "phi3.5:latest", or vice versa).
export function pickLoaded(
  byName: Map<string, LoadedModel>,
  model: string,
): LoadedModel | undefined {
  const base = model.replace(/:latest$/, "");
  return byName.get(model) ?? byName.get(base) ?? byName.get(`${base}:latest`);
}

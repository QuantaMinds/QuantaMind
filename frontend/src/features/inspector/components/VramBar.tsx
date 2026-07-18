import type { LoadedModel } from "../../../shared/ipc/system/vram";
import type { HardwareSnapshot } from "../../../shared/ipc/compare/hardware";
import { formatBytes } from "../../../shared/format/bytes";
import { PRESSURE_FRACTION, PRESSURE_LABEL } from "../../../shared/memory/pressure";
import { vramUsage } from "../format/vram";

/// Per-model memory bar: the model's resident footprint as a share of the
/// device memory pool (unified RAM on Apple, VRAM total on NVIDIA).
/// CLI telemetry console styled.
export function VramBar({
  entry,
  deviceTotalBytes,
  unified,
  hw,
}: {
  entry?: LoadedModel;
  deviceTotalBytes?: number | null;
  unified?: boolean;
  hw?: HardwareSnapshot | null;
}) {
  if (!entry) {
    return (
      <div className="text-xs text-gray-400 font-mono" data-testid="vram-na">
        VRAM not available — load the model to measure
      </div>
    );
  }

  const u = vramUsage(entry.size_bytes, entry.size_vram_bytes, deviceTotalBytes, unified);
  const where = unified ? "in unified memory" : "in VRAM";
  const ctx = entry.context_length;
  const tip = ctx ? `Full ${ctx}-token KV cache is preallocated into memory at load` : undefined;

  // Calculate base system memory load excluding the active model
  const totalBytes = u.totalBytes;
  const modelBytes = entry.size_vram_bytes || entry.size_bytes || 0;
  let systemBaseBytes = 0;

  if (hw) {
    if (unified) {
      const usedSys = hw.total_memory_bytes - hw.available_memory_bytes;
      systemBaseBytes = Math.max(0, usedSys - modelBytes);
    } else if (hw.gpu && hw.gpu.vram_total_bytes != null && hw.gpu.vram_free_bytes != null) {
      // Only derive "system" usage when BOTH VRAM totals are actually reported —
      // a missing free value would otherwise inflate it into a fabricated figure.
      const usedVram = hw.gpu.vram_total_bytes - hw.gpu.vram_free_bytes;
      systemBaseBytes = Math.max(0, usedVram - modelBytes);
    }
  }

  const modelPct = totalBytes > 0 ? (modelBytes / totalBytes) * 100 : 0;
  const systemPct = totalBytes > 0 ? (systemBaseBytes / totalBytes) * 100 : 0;

  const totalCells = 50;
  const modelCells = Math.round((modelPct / 100) * totalCells);
  const systemCells = Math.round((systemPct / 100) * totalCells);

  return (
    <div className="text-[11px] font-mono space-y-1" data-testid="vram-bar" title={tip}>
      {/* Hidden assertions for unit tests */}
      <div data-testid="vram-seg-used" className="hidden" style={{ width: `${u.pct}%` }} />

      <div className="text-gray-500 font-semibold tracking-wider text-[10px] uppercase">
        UNIFIED MEMORY (VRAM)
      </div>
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex items-center text-blue-600 select-none font-mono tracking-tighter text-sm">
          <span className="text-gray-500">[</span>
          {Array.from({ length: totalCells }).map((_, i) => {
            let color = "text-gray-400";
            let char = "░";
            if (i < modelCells) {
              color = "text-blue-600";
              char = "█";
            } else if (i < modelCells + systemCells) {
              color = "text-gray-500";
              char = "░";
            }

            // The backend's PRESSURE_FRACTION planning threshold — a soft
            // "Conditional" marker, NOT a measured OOM point (nothing measures
            // where this machine OOMs), so it renders amber and static.
            const isPressureMarker = i === Math.floor(totalCells * PRESSURE_FRACTION);

            return (
              <span key={i} className={`relative inline-block w-[7px] text-center ${color}`}>
                {char}
                {isPressureMarker && (
                  <span
                    className="absolute inset-y-0 left-0 w-[2px] bg-amber-500 z-10"
                    title={PRESSURE_LABEL}
                  />
                )}
              </span>
            );
          })}
          <span className="text-gray-500">]</span>
        </div>
        <div className="text-gray-600 text-xs">
          {formatBytes(u.usedBytes)} {where} of {formatBytes(u.totalBytes)} ({u.pct.toFixed(0)}%)
          {u.offloadBytes > 0 ? ` · ${formatBytes(u.offloadBytes)} offloaded to RAM` : ""}
          {ctx ? ` · ${ctx} ctx` : ""}
        </div>
      </div>

      {/* Caption under the bar (pressure marker shown inline in the bar above) */}
      <div className="flex justify-between gap-4 text-[10px] text-gray-500" style={{ maxWidth: "370px" }}>
        <span>▲ Model load ({formatBytes(modelBytes)})</span>
        <span className="text-amber-600 whitespace-nowrap" title={PRESSURE_LABEL}>
          ▲ pressure threshold (planning estimate)
        </span>
      </div>
    </div>
  );
}

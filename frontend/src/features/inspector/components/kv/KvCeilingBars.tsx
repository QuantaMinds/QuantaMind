import { useCliffStore } from "../../../eval/state/cliffStore";
import { useKvCeilings } from "../../hooks/useKvCeilings";
import type { BackendKind } from "../../../../shared/ipc/models/storage";

const TOTAL_CELLS = 50;

/// One 50-cell monospace ceiling bar for a KV precision, in the ContextBudgetBar
/// style. Width is `ceiling / scaleMax`; a cliff marker (from the cliff store)
/// and a "model max" tick overlay it. `ceiling === null` → "Not available".
function CeilingRow({
  label,
  ceiling,
  scaleMax,
  cliff,
  modelMax,
  accent,
}: {
  label: string;
  ceiling: number | null;
  scaleMax: number;
  cliff: number | null;
  modelMax: number | null;
  accent: string;
}) {
  if (ceiling == null) {
    return (
      <div className="flex items-center gap-2 text-[11px] font-mono" data-testid={`kv-ceiling-${label}`}>
        <span className="w-8 text-gray-500 uppercase">{label}</span>
        <span className="text-gray-400">Not available</span>
      </div>
    );
  }
  // The bar tops out at the model's own declared max — you can't reach more
  // context than the weights support, however much memory is free.
  const usable = modelMax ? Math.min(ceiling, modelMax) : ceiling;
  const filled = Math.min(TOTAL_CELLS, Math.round((usable / scaleMax) * TOTAL_CELLS));
  const cliffCell = cliff && scaleMax > 0 ? Math.min(TOTAL_CELLS - 1, Math.round((cliff / scaleMax) * TOTAL_CELLS)) : null;
  const maxCell = modelMax && modelMax < scaleMax ? Math.round((modelMax / scaleMax) * TOTAL_CELLS) : null;
  const clamped = modelMax != null && ceiling > modelMax;

  return (
    <div className="flex items-center gap-2 text-[11px] font-mono" data-testid={`kv-ceiling-${label}`}>
      <span className="w-8 text-gray-500 uppercase">{label}</span>
      <span className="flex items-center tracking-tighter text-sm select-none">
        <span className="text-gray-500">[</span>
        {Array.from({ length: TOTAL_CELLS }).map((_, i) => {
          const on = i < filled;
          return (
            <span key={i} className={`relative inline-block w-[7px] text-center ${on ? accent : "text-gray-300"}`}>
              {on ? "█" : "░"}
              {i === cliffCell && (
                <span
                  className="absolute inset-y-0 left-0 w-[2px] bg-red-500 z-10"
                  title={`Indicative cliff edge ≈${cliff} ctx`}
                />
              )}
              {i === maxCell && (
                <span className="absolute inset-y-0 left-0 w-[2px] bg-gray-500 z-10" title={`Model max ${modelMax} ctx`} />
              )}
            </span>
          );
        })}
        <span className="text-gray-500">]</span>
      </span>
      <span className="text-gray-600 text-xs whitespace-nowrap">
        ≈{usable.toLocaleString()} ctx{clamped ? " (model max)" : ""}
      </span>
    </div>
  );
}

/// "Context ceiling by KV cache precision" — three bars (f16/q8_0/q4_0) showing
/// how much context THIS machine holds for the model at each cache precision.
/// f16 is the default; q8_0 ≈ doubles it at negligible quality cost; q4_0 ≈
/// quadruples it but carries a real quality cost AND can be much slower at long
/// context, so QuantaMind never auto-launches a Q4 cache — it's planning info.
export function KvCeilingBars({
  modelName,
  backend,
  modelBytes,
  totalBytes,
  unified,
}: {
  modelName: string;
  backend: BackendKind | undefined;
  modelBytes: number | null | undefined;
  totalBytes: number | null | undefined;
  unified?: boolean;
}) {
  const { dims, ceilings } = useKvCeilings(modelName, backend, modelBytes, totalBytes);
  const cliff = useCliffStore((s) => s.cliffForModel(modelName));
  const modelMax = dims?.context_length || null;
  const estimated = dims?.kv_estimated ?? false;

  if (!ceilings) {
    // Backend-aware reason (the old copy always said "needs an Ollama model", misleading on
    // llama.cpp). For llama.cpp the dims come from the GGUF and the size from the LOADED server,
    // so a missing measurement almost always means the model isn't loaded.
    const naReason =
      backend === "llama_cpp"
        ? dims
          ? "load the model in llama.cpp to measure"
          : "GGUF dimensions unavailable"
        : backend === "ollama"
          ? "needs an Ollama model with reported dimensions"
          : "not measurable for this backend";
    return (
      <div className="text-[11px] font-mono space-y-1" data-testid="kv-ceilings">
        <div className="text-gray-500 font-semibold tracking-wider text-[10px] uppercase">
          CONTEXT CEILING BY KV CACHE PRECISION
        </div>
        <div className="text-gray-400" data-testid="kv-ceilings-na">Not available ({naReason})</div>
      </div>
    );
  }

  // Shared x-scale so the three bars are visually comparable: the largest
  // reachable context (q4), itself capped at the model's declared max.
  const rawMax = Math.max(ceilings.f16 ?? 0, ceilings.q8 ?? 0, ceilings.q4 ?? 0, 1);
  const scaleMax = modelMax ? Math.min(rawMax, modelMax) : rawMax;
  const memWord = unified ? "unified memory" : "VRAM";

  return (
    <div className="text-[11px] font-mono space-y-1.5" data-testid="kv-ceilings">
      <div className="text-gray-500 font-semibold tracking-wider text-[10px] uppercase flex items-center gap-1.5">
        CONTEXT CEILING BY KV CACHE PRECISION
        {estimated && (
          <span className="text-amber-600 normal-case font-normal" title="This model didn't report its KV head count, so the cache is over-estimated — the ceilings are conservative (they under-promise context, never over-promise).">
            ~ estimated
          </span>
        )}
      </div>
      <CeilingRow label="f16" ceiling={ceilings.f16} scaleMax={scaleMax} cliff={cliff} modelMax={modelMax} accent="text-green-600" />
      <CeilingRow label="q8" ceiling={ceilings.q8} scaleMax={scaleMax} cliff={cliff} modelMax={modelMax} accent="text-blue-600" />
      <CeilingRow label="q4" ceiling={ceilings.q4} scaleMax={scaleMax} cliff={cliff} modelMax={modelMax} accent="text-amber-600" />
      <div className="text-[10px] text-gray-500 leading-snug pt-0.5">
        How much context fits in {memWord} at each cache precision. q8_0 ≈ 2× f16 at negligible quality cost;
        q4_0 ≈ 4× but with a real quality cost <span className="text-amber-600">and can be much slower at long context</span> —
        a planning estimate, QuantaMind never auto-launches a q4_0 cache.
      </div>
    </div>
  );
}

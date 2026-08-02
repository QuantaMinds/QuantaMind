import { useEffect, useState } from "react";
import type { BatchColumn } from "../../../../shared/ipc/eval/batch";
import type { BackendKind } from "../../../../shared/ipc/models/storage";
import { estimateKvCacheBytes } from "../../../../shared/ipc/system/inspect";
import { formatBytes } from "../../../../shared/format/bytes";
import { fitOfNeed, fitBadge } from "../../../models/fit";
import { deviceMemory, useHardware } from "../../hooks/useHardware";
import { useKvCeilings } from "../../hooks/useKvCeilings";
import { KvCeilingBars } from "../kv/KvCeilingBars";
import { InfoButton } from "../../../../shared/ui/InfoButton";
import { EVAL_RUN_HELP } from "./evalRunHelp";

/// A row of the stacked memory breakdown. Provenance is part of the row, not decoration:
/// every number names how it was obtained (measured / computed from measured tokens /
/// estimated / claimed) — the review rule this panel exists to enforce.
function Row({ label, value, provenance, strong }: { label: string; value: string | null; provenance: string; strong?: boolean }) {
  return (
    <div className="flex flex-col sm:flex-row sm:items-baseline justify-between py-2 border-b border-slate-100 last:border-0 gap-1 sm:gap-4">
      <div className="flex flex-col">
        <span className={strong ? "text-sm font-semibold text-slate-900" : "text-sm font-medium text-slate-700"}>{label}</span>
        <span className="text-[10px] uppercase tracking-wider text-slate-400 font-medium">{provenance}</span>
      </div>
      <span className={`text-sm font-mono ${value == null ? "text-slate-400" : strong ? "font-bold text-indigo-600" : "text-slate-700"}`}>
        {value ?? "N/A"}
      </span>
    </div>
  );
}

/// "How much memory does this agent task cost on this box?" — model footprint (constant
/// baseline) + KV for THIS run (the per-task headline) + process RSS (diagnostic only).
/// The headline is never process RSS: RSS includes weights + residue and would answer a
/// different question than the one asked. Data flows through the SAME hooks the workspace
/// meters use (`useKvCeilings`/`useHardware` — no one-shot ref guards; StrictMode's dev
/// double-mount silently starved the previous bespoke fetch).
export function MemoryEstimatePanel({
  model,
  backend,
  column,
  peakTokens,
  contextWindow,
  kvMeasured,
  maxRssBytes,
  oomTaskId,
}: {
  model: string;
  backend: BackendKind | undefined;
  column: BatchColumn | undefined;
  peakTokens: number | null;
  /// The window the run ACTUALLY ran under (column ctx_ceiling, else the report's num_ctx,
  /// else a truncated step's context_window) — the denominator of the budget line.
  contextWindow: number | null;
  kvMeasured: boolean;
  maxRssBytes: number | null;
  oomTaskId: string | null;
}) {
  const hw = useHardware();
  const device = deviceMemory(hw);
  const weightsTotal = column?.weights_total_bytes ?? null;
  // Same fetch pattern as the workspace KV meters (cancel-on-cleanup, refetch on remount).
  const { dims, ceilings } = useKvCeilings(model, backend, weightsTotal, hw?.total_memory_bytes, device.workingSetBytes);
  const [kvAll, setKvAll] = useState<{ f16: number; q8: number; q4: number } | null>(null);

  // KV bytes at the run's peak token occupancy, at ALL three cache precisions — the same
  // canonical Rust formula each time (exact integer scaling; never halved in JS). The
  // headline shows the precision the run actually used; the sub-line shows what the SAME
  // tokens would cost at the others (the "would a quantized cache save me?" decision).
  const kvType = column?.kv_cache_type === "q8_0" ? "q8_0" : undefined;
  useEffect(() => {
    let cancelled = false;
    if (!dims || peakTokens == null || peakTokens === 0) {
      setKvAll(null);
      return;
    }
    Promise.all([
      estimateKvCacheBytes(dims, peakTokens, "f16"),
      estimateKvCacheBytes(dims, peakTokens, "q8_0"),
      estimateKvCacheBytes(dims, peakTokens, "q4_0"),
    ])
      .then(([f16, q8, q4]) => !cancelled && setKvAll({ f16, q8, q4 }))
      .catch(() => !cancelled && setKvAll(null));
    return () => {
      cancelled = true;
    };
  }, [dims, peakTokens]);
  const kvBytes = kvAll == null ? null : kvType === "q8_0" ? kvAll.q8 : kvAll.f16;

  const weightsVram = column?.weights_vram_bytes ?? null;
  // The model figure, from whichever measurement this backend HAS: the resident
  // size_vram, else the llama.cpp launch's on-disk GGUF size (no resident split exists
  // there) — the provenance label names which one is showing.
  const modelBytes = weightsVram ?? weightsTotal;
  const modelProvenance =
    weightsVram != null
      ? "measured (/api/ps size_vram — weights + the KV/context buffer reserved at load, so it reads above the raw weight file)"
      : "measured (GGUF size at launch — llama.cpp reports no resident/VRAM split)";
  const offload = column?.cpu_offloaded ? column?.offload_bytes ?? null : null;
  // The verdict compares the WORKLOAD (model + this run's KV) against the DEVICE POOL —
  // never against currently-available memory: with the model already resident, "available"
  // has the model's own footprint subtracted, and comparing need against it double-counts
  // the weights (it declared "Won't fit" on a run that was literally executing).
  const pool = device.totalBytes;
  const need = modelBytes != null && kvBytes != null ? modelBytes + kvBytes : null;
  const verdict = need != null && pool != null ? fitBadge(fitOfNeed(need, pool)) : null;
  const kvTier = kvMeasured ? "computed from measured tokens (llama.cpp)" : "estimated (formula)";
  const kvApprox = dims?.kv_estimated ? "~" : "";
  const budgetPct = peakTokens != null && contextWindow ? Math.round((peakTokens / contextWindow) * 100) : null;

  return (
    <div className="bg-white border border-slate-200 rounded-xl shadow-sm overflow-hidden mb-6" data-testid="eval-memory-panel">
      <div className="flex items-center justify-between px-4 py-3 bg-slate-50 border-b border-slate-200">
        <div className="flex items-center gap-2 text-xs font-bold uppercase tracking-wider text-slate-600">
          <svg className="w-4 h-4 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m14-6h2m-2 6h2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z" />
          </svg>
          Hardware Footprint <span className="text-slate-400 font-mono font-normal normal-case">— {model}</span>
        </div>
        <InfoButton title={EVAL_RUN_HELP.memory.title} body={EVAL_RUN_HELP.memory.body} align="right" testId="eval-memory" />
      </div>
      <div className="p-4 space-y-1">
      <Row
        label="Model in memory"
        value={modelBytes != null ? formatBytes(modelBytes) : null}
        provenance={modelProvenance}
      />
      {offload != null && offload > 0 && (
        <Row label="Spilled to CPU" value={formatBytes(offload)} provenance="measured (size − size_vram) — the slow-inference cause" />
      )}
      <Row
        label="KV cache at this run's peak"
        value={kvBytes != null ? `${kvApprox}${formatBytes(kvBytes)}${peakTokens != null ? ` @ ${peakTokens} tok` : ""}` : null}
        provenance={`${kvTier} · ${
          column?.kv_cache_type
            ? `${column.kv_cache_type} KV (launched)`
            : "at f16 KV — the default; this backend doesn't report its actual cache type"
        }`}
        strong
      />
      {kvAll != null && (
        <div className="text-[11px] text-gray-500 pl-2" data-testid="eval-kv-precisions">
          same {peakTokens} tokens by cache precision: f16 {kvApprox}{formatBytes(kvAll.f16)} · q8_0 {kvApprox}
          {formatBytes(kvAll.q8)} · q4_0 {kvApprox}{formatBytes(kvAll.q4)} — cache type is a LAUNCH setting (q4_0 has a
          real quality cost; see the ceiling meters below)
        </div>
      )}
      <Row
        label="Server process RSS"
        value={maxRssBytes != null ? formatBytes(maxRssBytes) : null}
        provenance="diagnostic — max of step-end samples; whole process, and GPU-wired buffers may not appear here (it can read BELOW the model's in-memory size)"
      />
      {peakTokens != null && contextWindow != null && (
        <div className="mt-3 p-3 bg-slate-50 rounded-lg border border-slate-100 flex items-center justify-between" data-testid="eval-ctx-budget">
          <div className="flex flex-col">
            <span className="text-[10px] uppercase font-bold tracking-wider text-slate-500">Context Window Budget</span>
            <span className="text-xs text-slate-500 mt-0.5">Peak tokens vs launched window. (Precision-independent)</span>
          </div>
          <div className="text-sm font-mono font-semibold text-slate-700">
            {peakTokens} <span className="text-slate-400 font-normal">/ {contextWindow} ctx</span>
            {budgetPct != null && <span className="ml-2 text-indigo-600 bg-indigo-50 border border-indigo-100 px-1.5 py-0.5 rounded">{budgetPct}%</span>}
          </div>
        </div>
      )}
      {verdict && (
        <div className="text-sm pt-1" data-testid="eval-memory-verdict">
          <span className={`font-semibold ${verdict.cls}`}>{verdict.text}</span>{" "}
          <span className="text-[11px] text-gray-400">
            model + this run's KV ({need != null ? formatBytes(need) : "?"}) vs the {pool != null ? formatBytes(pool) : "?"}{" "}
            {device.unified ? "unified-memory" : "VRAM"} pool — planning estimate ("Tight" above 70% of the pool), not a measured OOM point
          </span>
        </div>
      )}
      {column?.quantization_claimed && (
        <div className="text-[11px] text-gray-400">Quantization (tag's claim, unverified): {column.quantization_claimed}</div>
      )}
      <div className="pt-1">
        <KvCeilingBars
          modelName={model}
          backend={backend}
          modelBytes={weightsTotal}
          totalBytes={device.totalBytes}
          unified={device.unified}
          workingSetBytes={device.workingSetBytes}
        />
      </div>
      {oomTaskId != null && (
        <div className="text-sm text-rose-700 bg-rose-50 border border-rose-200 rounded-lg p-3 mt-3 shadow-sm" data-testid="eval-oom-answer">
          <div className="font-semibold flex items-center gap-2">
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
            Out of memory during \u201C{oomTaskId}\u201D.
          </div>
          <div className="mt-2 text-rose-600/90 pl-6">
          {ceilings ? (
            <div className="space-y-1">
              <div>Fits on this machine at context:</div>
              <ul className="list-disc list-inside font-mono text-xs">
                <li>≤ {ceilings.f16 ?? "—"} <span className="text-rose-500">(f16 KV)</span></li>
                {ceilings.q8 != null && <li>≤ {ceilings.q8} <span className="text-rose-500">(q8_0 KV)</span></li>}
                {ceilings.q4 != null && <li>≤ {ceilings.q4} <span className="text-rose-500">(q4_0 KV)</span></li>}
              </ul>
              {dims?.kv_estimated && <div className="mt-1 text-[11px] italic">— conservative estimate (model didn't report kv heads)</div>}
            </div>
          ) : (
            <div>
              No safe-context suggestion: the model's dimensions aren't readable, and a guessed ceiling would be worse than none.
            </div>
          )}
          </div>
        </div>
      )}
      </div>
    </div>
  );
}

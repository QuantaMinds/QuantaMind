import { useEffect, useState } from "react";
import type { BatchColumn } from "../../../../shared/ipc/eval/batch";
import type { BackendKind } from "../../../../shared/ipc/models/storage";
import { estimateKvCacheBytes } from "../../../../shared/ipc/system/inspect";
import { formatBytes } from "../../../../shared/format/bytes";
import { fitOfNeed, fitBadge } from "../../../models/fit";
import { deviceMemory, useHardware } from "../../hooks/useHardware";
import { useKvCeilings } from "../../hooks/useKvCeilings";
import { KvCeilingBars } from "../kv/KvCeilingBars";

/// A row of the stacked memory breakdown. Provenance is part of the row, not decoration:
/// every number names how it was obtained (measured / computed from measured tokens /
/// estimated / claimed) — the review rule this panel exists to enforce.
function Row({ label, value, provenance, strong }: { label: string; value: string | null; provenance: string; strong?: boolean }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className={strong ? "text-sm font-semibold text-slate-800" : "text-sm text-slate-700"}>{label}</span>
      <span className={`text-sm ${value == null ? "text-gray-400" : strong ? "font-semibold text-slate-900" : "text-slate-800"}`}>
        {value ?? "Not available"}
      </span>
      <span className="text-[11px] text-gray-400">{provenance}</span>
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
  const { dims, ceilings } = useKvCeilings(model, backend, weightsTotal, hw?.total_memory_bytes);
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
  // The model figure, from whichever measurement this backend HAS: Ollama's resident
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
    <div className="border border-slate-200 rounded-lg p-3 space-y-1.5" data-testid="eval-memory-panel">
      <div className="text-[11px] uppercase tracking-wide text-gray-400">Memory for this run — {model}</div>
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
        <div className="text-[11px] font-mono text-gray-600 pt-1" data-testid="eval-ctx-budget">
          <span className="text-gray-500 font-semibold tracking-wider text-[10px] uppercase">Context window budget </span>
          {peakTokens} / {contextWindow} ctx{budgetPct != null ? ` (${budgetPct}%)` : ""} — peak of a single run vs the window this
          run launched with. Tokens, so precision-independent: f16/q8/q4 change the BYTES per cached token, never this count.
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
        />
      </div>
      {oomTaskId != null && (
        <div className="text-sm text-red-700 bg-red-50 rounded p-2 mt-1" data-testid="eval-oom-answer">
          <div className="font-semibold">Out of memory during “{oomTaskId}”.</div>
          {ceilings ? (
            <div>
              Fits on this machine at context ≤ {ceilings.f16 ?? "—"} (f16 KV)
              {ceilings.q8 != null ? ` · ≤ ${ceilings.q8} (q8_0 KV)` : ""}
              {ceilings.q4 != null ? ` · ≤ ${ceilings.q4} (q4_0 KV)` : ""}
              {dims?.kv_estimated ? " — conservative estimate (model didn't report kv heads)" : ""}
            </div>
          ) : (
            <div className="text-red-600/80">
              No safe-context suggestion: the model's dimensions aren't readable, and a guessed ceiling would be worse than none.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

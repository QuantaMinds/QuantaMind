import { useEffect, useRef, useState } from "react";
import type { BatchColumn } from "../../../../shared/ipc/eval/batch";
import type { BackendKind } from "../../../../shared/ipc/models/storage";
import {
  contextCeilings,
  estimateKvCacheBytes,
  inspectModel,
  type CtxCeilings,
  type ModelDims,
} from "../../../../shared/ipc/system/inspect";
import { getHardwareSnapshot, type HardwareSnapshot } from "../../../../shared/ipc/compare/hardware";
import { formatBytes } from "../../../../shared/format/bytes";
import { fitOfNeed, fitBadge } from "../../../models/fit";

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

/// "How much memory does this agent task cost on this box?" — weights (constant baseline)
/// + KV for THIS run (the per-task headline) + process RSS (diagnostic only). The headline
/// is never process RSS: RSS includes weights + residue and would answer a different
/// question than the one asked.
export function MemoryEstimatePanel({
  model,
  backend,
  column,
  peakTokens,
  kvMeasured,
  maxRssBytes,
  oomTaskId,
}: {
  model: string;
  backend: BackendKind | undefined;
  column: BatchColumn | undefined;
  peakTokens: number | null;
  kvMeasured: boolean;
  maxRssBytes: number | null;
  oomTaskId: string | null;
}) {
  const [dims, setDims] = useState<ModelDims | null>(null);
  const [kvBytes, setKvBytes] = useState<number | null>(null);
  const [hw, setHw] = useState<HardwareSnapshot | null>(null);
  const [ceilings, setCeilings] = useState<CtxCeilings | null>(null);
  // One-shot per (model, backend): dims/hardware don't change mid-run.
  const fetchedFor = useRef<string | null>(null);

  useEffect(() => {
    if (!backend) return; // backend unknown → never guess a dims source
    const key = `${model}\0${backend}`;
    if (fetchedFor.current === key) return;
    fetchedFor.current = key;
    let alive = true;
    void getHardwareSnapshot().then((s) => alive && setHw(s)).catch(() => {});
    void inspectModel(model, backend)
      .then((m) => alive && setDims(m.dims))
      .catch(() => alive && setDims(null));
    return () => {
      alive = false;
    };
  }, [model, backend]);

  // KV bytes at the run's peak token occupancy, at the KV precision the run actually used
  // (llama-server q8_0 when launched that way; f16 otherwise). Recomputed when the peak grows.
  useEffect(() => {
    if (!dims || peakTokens == null || peakTokens === 0) return;
    let alive = true;
    const precision = column?.kv_cache_type === "q8_0" ? "q8_0" : undefined;
    void estimateKvCacheBytes(dims, peakTokens, precision)
      .then((b) => alive && setKvBytes(b))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [dims, peakTokens, column?.kv_cache_type]);

  // The actionable OOM answer — only when an OOM actually happened AND dims are known
  // (no dims → no suggestion; a fabricated ceiling is worse than none).
  const weightsTotal = column?.weights_total_bytes ?? null;
  useEffect(() => {
    if (!dims || !hw || oomTaskId == null || weightsTotal == null) return;
    let alive = true;
    void contextCeilings(dims, weightsTotal, hw.total_memory_bytes)
      .then((c) => alive && setCeilings(c))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [dims, hw, oomTaskId, weightsTotal]);

  const weightsVram = column?.weights_vram_bytes ?? null;
  const offload = column?.cpu_offloaded ? column?.offload_bytes ?? null : null;
  const avail = hw?.available_memory_bytes ?? null;
  const need = weightsVram != null && kvBytes != null ? weightsVram + kvBytes : null;
  const verdict = need != null && avail != null ? fitBadge(fitOfNeed(need, avail)) : null;
  const kvTier = kvMeasured ? "computed from measured tokens (llama.cpp)" : "estimated (formula)";
  const kvApprox = dims?.kv_estimated ? "~" : "";

  return (
    <div className="border border-slate-200 rounded-lg p-3 space-y-1.5" data-testid="eval-memory-panel">
      <div className="text-[11px] uppercase tracking-wide text-gray-400">Memory for this run — {model}</div>
      <Row
        label="Weights in memory"
        value={weightsVram != null ? formatBytes(weightsVram) : null}
        provenance="measured (/api/ps size_vram)"
      />
      {offload != null && offload > 0 && (
        <Row label="Spilled to CPU" value={formatBytes(offload)} provenance="measured (size − size_vram) — the slow-inference cause" />
      )}
      <Row
        label="KV cache at this run's peak"
        value={kvBytes != null ? `${kvApprox}${formatBytes(kvBytes)}${peakTokens != null ? ` @ ${peakTokens} tok` : ""}` : null}
        provenance={`${kvTier}${column?.kv_cache_type ? ` · ${column.kv_cache_type} KV` : ""}`}
        strong
      />
      <Row
        label="Server process RSS"
        value={maxRssBytes != null ? formatBytes(maxRssBytes) : null}
        provenance="diagnostic — max of step-end samples; includes weights + residue, never a per-task amount"
      />
      {verdict && (
        <div className="text-sm pt-1" data-testid="eval-memory-verdict">
          <span className={`font-semibold ${verdict.cls}`}>{verdict.text}</span>{" "}
          <span className="text-[11px] text-gray-400">
            weights + KV vs {avail != null ? formatBytes(avail) : "?"} available — planning estimate, not a measured OOM point
          </span>
        </div>
      )}
      {column?.quantization_claimed && (
        <div className="text-[11px] text-gray-400">Quantization (tag's claim, unverified): {column.quantization_claimed}</div>
      )}
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

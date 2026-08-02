import type { GenerateStats } from "../../../shared/ipc/events/events";
import { buildTtftSegments } from "../format/ttft";
import { cacheReuse } from "../../../shared/format/cache";

// Phase palette — kept distinct from the token-event colors (amber TTFT / blue
// gap / red outlier) so a colour means one thing across both inspector charts.
const COLOR: Record<string, string> = {
  load: "#64748b", // slate-500 — model load
  prefill: "#7c3aed", // violet-600 — prompt prefill
  remainder: "#16a34a", // green-600 — generation / stream & gen
};

/// Stacked horizontal bar decomposing the measured run duration into
/// Model load + Prompt prefill + Stream & Gen.
/// Aligns with the shared X-axis of the model console.
export function TtftBreakdown({
  ttftMs,
  stats,
  maxTime,
  marginLeft = 60,
  marginRight = 150,
  width = 640,
}: {
  ttftMs: number | null;
  stats?: GenerateStats;
  maxTime?: number;
  marginLeft?: number;
  marginRight?: number;
  width?: number;
}) {
  const { available, promptTokens } = buildTtftSegments(ttftMs, stats);

  if (!available) {
    return (
      <div className="text-xs text-gray-400 font-mono" data-testid="ttft-na">
        TTFT breakdown not available for this backend
      </div>
    );
  }

  const loadMs = stats?.load_ms ?? 0;
  const prefillMs = stats?.prompt_eval_ms ?? 0;
  const totalDuration = maxTime || Math.max(stats?.total_ms ?? 0, ttftMs ?? 0) || 1;
  const remainderMs = Math.max(0, totalDuration - loadMs - prefillMs);

  // Derived prefill throughput. Guard the 0/0 case: a full prefix-cache hit yields
  // prompt_eval_ms ≈ 0 AND prompt_eval_count ≈ 0 — render "cache hit, no prefill"
  // rather than NaN/∞. Otherwise tokens ÷ seconds. `null` when counts are unknown.
  const prefillTps =
    promptTokens == null
      ? null
      : promptTokens === 0 || prefillMs === 0
        ? "cache hit — no prefill"
        : `${Math.round(promptTokens / (prefillMs / 1000)).toLocaleString()} tok/s prefill`;

  const loadPct = (loadMs / totalDuration) * 100;
  const prefillPct = (prefillMs / totalDuration) * 100;
  const remainderPct = (remainderMs / totalDuration) * 100;

  const innerWidth = Math.max(0, width - marginLeft - marginRight);

  // Define segments for rendering and testing compatibility
  const segments = [
    { key: "load", label: "Model load", ms: loadMs, pct: loadPct },
    { key: "prefill", label: "Prompt prefill", ms: prefillMs, pct: prefillPct },
    { key: "remainder", label: "Stream & Gen", ms: remainderMs, pct: remainderPct },
  ];

  return (
    <div className="space-y-1 font-mono text-xs select-none" data-testid="ttft-breakdown">
      {/* Align Phase Labels above the timeline */}
      <div
        className="flex text-[10px] text-gray-500 font-semibold tracking-wider"
        style={{ marginLeft: `${marginLeft}px`, width: `${innerWidth}px` }}
      >
        {loadMs > 0 && (
          <div style={{ width: `${loadPct}%` }} className="truncate" title={`Model Load: ${loadMs}ms`}>
            [ 1. Model Load ]
          </div>
        )}
        {prefillMs > 0 && (
          <div style={{ width: `${prefillPct}%` }} className="truncate" title={`Prompt Prefill: ${prefillMs}ms`}>
            [ 2. Prompt Prefill ]
          </div>
        )}
        {remainderMs > 0 && (
          <div style={{ width: `${remainderPct}%` }} className="truncate" title={`Stream & Gen: ${remainderMs}ms`}>
            [ 3. Stream & Gen ]
          </div>
        )}
      </div>

      {/* Aligned Timeline Bar */}
      <div className="flex items-center text-gray-500">
        <div
          style={{ width: `${marginLeft}px` }}
          className="text-right pr-2 text-[10px] font-semibold text-gray-500"
        >
          0ms
        </div>
        <div
          className="flex h-3 overflow-hidden rounded bg-gray-100 border border-gray-400"
          style={{ width: `${innerWidth}px` }}
        >
          {segments.map((s) => {
            if (s.ms <= 0) return null;
            return (
              <div
                key={s.key}
                data-testid={`ttft-seg-${s.key}`}
                title={`${s.label}: ${s.ms}ms`}
                style={{ width: `${s.pct}%`, background: COLOR[s.key] }}
              />
            );
          })}
        </div>
        <div
          style={{ width: `${marginRight}px` }}
          className="pl-2 text-[10px] font-semibold text-gray-500"
        >
          {Math.round(totalDuration)}ms
        </div>
      </div>

      {promptTokens != null && (
        <div
          className="text-[10px] text-gray-500 font-semibold"
          style={{ marginLeft: `${marginLeft}px` }}
        >
          · {promptTokens} prompt tokens{prefillTps ? ` · ${prefillTps}` : ""}
        </div>
      )}
      {/* llama.cpp-only prefix-cache reuse. `available` is false for a remote backend
          (cache_n null) → absent; a cold llama run (cache_n 0) honestly shows
          "0 reused / N recomputed" — a measured zero, not absence-of-feature. */}
      {(() => {
        const cr = cacheReuse(stats?.cache_n, stats?.prompt_eval_count);
        if (!cr.available) return null;
        return (
          <div
            className="text-[10px] text-gray-500 font-semibold"
            data-testid="ttft-prefix-cache"
            style={{ marginLeft: `${marginLeft}px` }}
          >
            · prefix cache: {cr.cached} reused / {cr.recomputed} recomputed
          </div>
        );
      })()}
    </div>
  );
}

import { useState } from "react";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import type { CompareRow } from "../../compare/state/compareRow";
import type { LoadedModel } from "../../../shared/ipc/system/vram";
import type { HistoryEntry } from "../../../shared/ipc/workspace/history";
import type { HardwareSnapshot } from "../../../shared/ipc/compare/hardware";
import { buildLatencyBars, type LatencyBar } from "../format/timeline";
import { buildHistogram } from "../format/histogram";
import { TokenTimeline } from "./TokenTimeline";
import { LatencyHistogram } from "./LatencyHistogram";
import { TtftBreakdown } from "./TtftBreakdown";
import { VramBar } from "./VramBar";
import { ContextBudgetBar } from "./ContextBudgetBar";
import { KvCeilingBars } from "./kv/KvCeilingBars";
import { ColdWarmPanel } from "./ColdWarmPanel";
import { RegressionAlert } from "./RegressionAlert";
import { useModelLabel } from "../../models/hooks/useModelLabel";
import { formatBytes } from "../../../shared/format/bytes";

const tag = (k: LatencyBar["kind"]) => (k === "ttft" ? " (TTFT)" : k === "outlier" ? " (outlier)" : "");

/// One model's timing: name + summary, a hover readout, and its bar chart.
export function ModelTimeline({
  row,
  width,
  vram,
  history = [],
  deviceTotalBytes,
  unified,
  workingSetBytes,
  hw,
}: {
  row: CompareRow;
  width: number;
  vram?: LoadedModel;
  history?: HistoryEntry[];
  deviceTotalBytes?: number | null;
  unified?: boolean;
  workingSetBytes?: number | null;
  hw?: HardwareSnapshot | null;
}) {
  const [hovered, setHovered] = useState<LatencyBar | null>(null);
  const label = useModelLabel();
  // The model's ACTUAL backend (llama.cpp reads KV dims from its GGUF header).
  // Resolved by name from the installed list — CompareRow doesn't carry it. Was hardcoded
  // a fixed value, which stranded llama.cpp → the KV meters said "Not
  // available".
  const backend = useInstalledModelsStore((s) => s.list).find((mm) => mm.name === row.model)?.backend;
  const m = row.metrics;
  const { bars, stats } = buildLatencyBars(m?.timeline ?? [], m?.ttft_ms ?? null);
  const histogram = buildHistogram(bars);
  const outliers = bars.filter((b) => b.kind === "outlier").length;
  const tps = m?.tokens_per_sec;

  // Alignments
  const marginLeft = 60;
  const marginRight = 150;

  // Time metrics
  const loadMs = m?.stats?.load_ms ?? 0;
  const prefillMs = m?.stats?.prompt_eval_ms ?? 0;
  const promptTokens = m?.stats?.prompt_eval_count ?? 0;
  const maxTime = Math.max(
    m?.stats?.total_ms ?? 0,
    m?.timeline?.[m.timeline.length - 1]?.t_ms ?? 0,
    m?.ttft_ms ?? 0
  );

  const totalDuration = maxTime || 1;
  const remainderMs = Math.max(0, totalDuration - loadMs - prefillMs);

  return (
    <div
      className="space-y-4 border border-gray-100 rounded-xl p-5 bg-gray-50 font-mono shadow-sm"
      data-testid={`model-timeline-${row.model}`}
    >
      {/* 1. Header/Target Info Panel */}
      <div className="flex flex-wrap items-baseline justify-between border-b border-gray-100 pb-2">
        <span className="text-sm font-semibold text-gray-900 flex items-center gap-1">
          <span>▼ TARGET:</span>
          <span>{label(row.model)}</span>
        </span>
        <span className="text-xs text-gray-500">
          {m?.token_count ?? "—"} tokens · TTFT {m?.ttft_ms != null ? `${m.ttft_ms}ms` : "—"} ·{" "}
          {tps != null ? `${tps.toFixed(1)} tok/s` : "— tok/s"} ·{" "}
          <span data-testid={`outliers-${row.model}`}>{outliers} outlier{outliers === 1 ? "" : "s"}</span>
        </span>
      </div>

      {/* 2. Timeline Phase track */}
      <TtftBreakdown
        ttftMs={m?.ttft_ms ?? null}
        stats={m?.stats}
        maxTime={maxTime}
        marginLeft={marginLeft}
        marginRight={marginRight}
        width={width || 640}
      />

      {/* 3. System Resource Budgets Console */}
      <div className="grid grid-cols-1 gap-5 border-t border-b border-gray-100 py-4 my-2">
        <VramBar entry={vram} deviceTotalBytes={deviceTotalBytes} unified={unified} hw={hw} />
        <ContextBudgetBar
          modelName={row.model}
          promptTokens={m?.stats?.prompt_eval_count ?? null}
          contextLength={vram?.context_length ?? null}
        />
        {/* Dims resolve per backend: llama.cpp reads them from its GGUF header. */}
        <KvCeilingBars
          modelName={row.model}
          backend={backend}
          modelBytes={vram?.size_bytes ?? null}
          totalBytes={deviceTotalBytes ?? null}
          unified={unified}
          workingSetBytes={workingSetBytes}
        />
      </div>

      {/* 4. Metric Cards */}
      <div className="flex flex-wrap gap-4 text-[11px] text-gray-500 font-semibold tracking-wider select-none my-2">
        {loadMs > 0 && (
          <div className="flex-1 min-w-[130px] border-r border-gray-100 pr-4">
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider">Cold Load</div>
            <div className="text-gray-700 text-sm font-semibold mt-1">{loadMs > 0 ? `${loadMs}ms` : "—"}</div>
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider mt-2">VRAM Usage</div>
            <div className="text-gray-700 text-xs font-semibold mt-1 truncate" title={vram ? formatBytes(vram.size_vram_bytes || vram.size_bytes) : "—"}>
              {vram ? formatBytes(vram.size_vram_bytes || vram.size_bytes) : "—"}
            </div>
          </div>
        )}
        {prefillMs > 0 && (
          <div className="flex-1 min-w-[130px] border-r border-gray-100 px-4">
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider">Prompt Prefill</div>
            <div className="text-gray-700 text-sm font-semibold mt-1">{prefillMs > 0 ? `${prefillMs}ms` : "—"}</div>
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider mt-2">Tokens</div>
            <div className="text-gray-700 text-sm font-semibold mt-1">
              {promptTokens > 0 ? `${promptTokens} prompt` : "—"}
            </div>
          </div>
        )}
        {remainderMs > 0 && (
          <div className="flex-1 min-w-[130px] pl-4">
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider">Inter-token</div>
            <div className="text-gray-700 text-sm font-semibold mt-1">{stats.meanMs > 0 ? `${stats.meanMs.toFixed(1)}ms` : "—"}</div>
            <div className="text-gray-500 uppercase text-[9px] font-semibold tracking-wider mt-2">Outliers</div>
            <div className="text-gray-700 text-sm font-semibold mt-1">{outliers} spikes</div>
          </div>
        )}
      </div>

      <ColdWarmPanel model={row.model} history={history} />
      <RegressionAlert model={row.model} history={history} />

      {/* 5. Hover detail readout */}
      <div className="text-[11px] text-gray-500 font-mono h-5 mt-2" data-testid={`readout-${row.model}`}>
        {hovered
          ? `#${hovered.index} · ${hovered.latencyMs}ms${tag(hovered.kind)} — ${JSON.stringify(hovered.token)}`
          : "Hover a bar for token detail"}
      </div>

      {/* 6. Token Latency SVG */}
      <TokenTimeline
        bars={bars}
        stats={stats}
        width={width || 640}
        height={140}
        hoveredIndex={hovered?.index ?? null}
        onHover={setHovered}
        maxTime={maxTime}
        loadMs={loadMs}
        prefillMs={prefillMs}
        ttftMs={m?.ttft_ms ?? null}
        marginLeft={marginLeft}
        marginRight={marginRight}
      />

      {histogram.length > 0 && (
        <div data-testid={`histogram-${row.model}`} className="border-t border-gray-100 pt-4 mt-2">
          <div className="text-[10px] uppercase tracking-wider text-gray-500 font-semibold mb-2">Latency distribution</div>
          <LatencyHistogram buckets={histogram} width={width || 640} height={90} />
        </div>
      )}
    </div>
  );
}

import { useEffect } from "react";
import { useCompareStore } from "../../../compare/state/compareStore";
import { useParentWidth } from "../../hooks/useParentWidth";
import { useLoadedModels } from "../../hooks/useLoadedModels";
import { useRunHistory } from "../../hooks/useRunHistory";
import { useHardware, deviceMemory } from "../../hooks/useHardware";
import { pickLoaded } from "../../format/vram";
import { ModelTimeline } from "../ModelTimeline";
import { ExportReportButton } from "../../report/ExportReportButton";

// Per-token event colours (the latency bars) and the run-phase colours (the
// breakdown track + the chart's vertical phase lines). Disjoint palettes so a
// colour means one thing across both charts.
const SWATCH = [
  { kind: "ttft", label: "TTFT", color: "#d97706" },
  { kind: "normal", label: "Token gap", color: "#2563eb" },
  { kind: "outlier", label: "Outlier (latency spike)", color: "#dc2626" },
];
const PHASE_SWATCH = [
  { label: "Model load", color: "#64748b" },
  { label: "Prompt prefill", color: "#7c3aed" },
  { label: "Generation", color: "#16a34a" },
];

/// The per-model timing panels (one `ModelTimeline` per charted run) plus the
/// shared colour legend and VRAM refresh. Reads the same compare rows both the
/// Latency (Inspector) view and the Analysis view render, so the metrics stay
/// byte-identical across both surfaces. Renders nothing when no run has a
/// timeline — the host owns its own empty state.
export function LatencyTimelines({
  active,
  showExport = false,
}: {
  active: boolean;
  showExport?: boolean;
}) {
  const rows = useCompareStore((s) => s.rows);
  const [ref, width] = useParentWidth<HTMLDivElement>();
  const { byName, refresh } = useLoadedModels();
  const { entries, refresh: refreshHistory } = useRunHistory();
  const hw = useHardware();
  const dev = deviceMemory(hw);
  // Re-read /api/ps + history whenever the hosting tab is (re)opened — the model
  // that just ran is loaded by then, so its VRAM footprint is fresh.
  useEffect(() => {
    if (active) {
      void refresh();
      void refreshHistory();
    }
  }, [active, refresh, refreshHistory]);
  const charted = rows.filter((r) => (r.metrics?.timeline?.length ?? 0) > 0);
  if (charted.length === 0) return null;

  return (
    <div className="space-y-4" data-testid="latency-timelines" ref={ref}>
      <div className="flex items-center justify-between">
        <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-500">
          <span className="text-gray-400">Phases:</span>
          {PHASE_SWATCH.map((s) => (
            <span key={s.label} className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded-sm" style={{ background: s.color }} />
              {s.label}
            </span>
          ))}
          <span className="text-gray-400 ml-1">Tokens:</span>
          {SWATCH.map((s) => (
            <span key={s.kind} className="flex items-center gap-1">
              <span className="inline-block h-2 w-2 rounded-sm" style={{ background: s.color }} />
              {s.label}
            </span>
          ))}
        </div>
        <div className="flex items-center gap-3">
          <button type="button" onClick={() => void refresh()}
            className="text-xs text-blue-600 hover:text-blue-800" data-testid="vram-refresh">
            Refresh VRAM
          </button>
          {showExport && <ExportReportButton />}
        </div>
      </div>
      {charted.map((row) => (
        <ModelTimeline key={row.model} row={row} width={width} vram={pickLoaded(byName, row.model)}
          history={entries} deviceTotalBytes={dev.totalBytes} unified={dev.unified} hw={hw} />
      ))}
    </div>
  );
}

import { useCompareStore } from "../state/compareStore";
import { useNavStore } from "../../../shared/state/navStore";
import { CompareColumn } from "./CompareColumn";
import { MetricsChart } from "./MetricsChart";
import { CompareDiff } from "./CompareDiff";
import { ExportButtons } from "./ExportButtons";
import { LatencyTimelines } from "../../inspector/components/timeline/LatencyTimelines";

/// Read-only results of the latest run: per-model output columns, throughput /
/// TTFT charts, a word-level diff (two models), and export. Model selection +
/// running live in the global header and the Workspace.
export function AnalysisTab() {
  const rows = useCompareStore((s) => s.rows);
  const topView = useNavStore((s) => s.topView);

  if (rows.length === 0) {
    return (
      <section data-testid="tab-analysis" className="space-y-2">
        <h2 className="text-lg font-semibold">Analysis</h2>
        <p data-testid="analysis-empty" className="text-sm text-gray-500">
          Run a model in the Workspace (or pick 2+ Ollama models in the header to
          compare) — then come here to compare throughput, time-to-first-token,
          and outputs.
        </p>
      </section>
    );
  }

  return (
    <section data-testid="tab-analysis" className="space-y-3">
      <h2 className="text-lg font-semibold">Analysis</h2>
      {rows.length > 0 && (
        <>
          <div className="flex gap-2 overflow-x-auto" data-testid="compare-columns">
            {rows.map((r) => <CompareColumn key={r.model} row={r} />)}
          </div>
          {/* Full per-token latency metrics for each answer above — the same panels the
              Latency (Inspector) tab shows, rendered here so they sit under the live answer. */}
          {/* showExport moved here when the Latency tab became Test-run-only — the
              workspace latency report exports from where its panels now live. */}
          <LatencyTimelines active={topView === "compare"} showExport />
          <MetricsChart />
          <CompareDiff />
          <ExportButtons />
        </>
      )}
    </section>
  );
}

import { useEffect, useState } from "react";
import { useEvalRegistryStore, DEFAULT_PRESET } from "../../eval/state/evalRegistryStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { loadCollectionHistory, type RunSummary } from "../../../shared/ipc/eval/matrix";
import { HistoryTimeline } from "../../eval/components/matrix/HistoryTimeline";
import { PresetOptGroups } from "../../eval/components/PresetOptGroups";
import { ContextCliffPanel } from "../../eval/components/ContextCliffPanel";
import { batchToCsv, download } from "../../eval/exportBatch";
import { InfoButton } from "../../../shared/ui/InfoButton";
import { TOOL_HELP } from "../../eval/help";

const exportBtn: React.CSSProperties = {
  padding: "6px 14px",
  borderRadius: 8,
  border: "1px solid #e2e8f0",
  background: "#ffffff",
  color: "#334155",
  fontSize: 12,
  fontFamily: "Inter,sans-serif",
  fontWeight: 600,
  cursor: "pointer",
};
const card: React.CSSProperties = {
  background: "#ffffff",
  border: "1px solid #e2e8f0",
  borderRadius: 12,
  padding: 16,
  boxShadow: "0 1px 3px 0 rgba(0, 0, 0, 0.05)",
};

/// The Audit tab (Zone 2 — compliance home): the saved Performance-Matrix
/// regression history, the audit-trail export of the latest batch run, and the
/// Context-Cliff diagnostic probe (moved off the Eval workspace).
export function AuditPage() {
  const { presets, collections, init } = useEvalRegistryStore();
  const report = useBatchStore((s) => s.report);
  const models = useInstalledModelsStore((s) => s.list);
  const selectedBackend = useBackendStore((s) => s.selectedBackend);
  const [collection, setCollection] = useState(DEFAULT_PRESET);
  const [history, setHistory] = useState<RunSummary[]>([]);
  // Show only the selected backend's regression history — a backend switch
  // shouldn't keep displaying the previous backend's model runs.
  const backendHistory = history.filter((h) => h.backend === selectedBackend);

  useEffect(() => {
    void init().catch((e) => console.error("eval registry init failed (AuditPage):", e));
  }, [init]);

  useEffect(() => {
    let cancelled = false;
    loadCollectionHistory(collection)
      .then((h) => !cancelled && setHistory(h))
      .catch(() => !cancelled && setHistory([]));
    return () => {
      cancelled = true;
    };
  }, [collection]);

  // Re-read the on-disk history whenever a batch finishes FOR THE SHOWN collection.
  // The backend appends to the history file before emitting `batch-complete`, so a
  // re-fetch here picks up the new run without an app restart. Guarding on the
  // collection id keeps a run for a different collection from clobbering the graph.
  useEffect(() => {
    if (!report || report.collection_id !== collection) return;
    let cancelled = false;
    loadCollectionHistory(collection)
      .then((h) => !cancelled && setHistory(h))
      .catch(() => !cancelled && setHistory([]));
    return () => {
      cancelled = true;
    };
  }, [report, collection]);

  return (
    <section data-testid="tab-audit" className="space-y-4">
      {/* The Context-Cliff probe sits on top; the Audit & Compliance history follows below it. */}
      <ContextCliffPanel />

      <h2 className="text-lg font-semibold text-slate-900">Audit &amp; Compliance</h2>

      <div style={card} data-testid="audit-history">
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
          <span style={{ fontSize: 13, fontWeight: 650, color: "#1e293b", fontFamily: "Inter,sans-serif" }}>Saved Matrix History</span>
          <select
            value={collection}
            onChange={(e) => setCollection(e.target.value)}
            data-testid="audit-collection"
            style={{ ...exportBtn, color: "#334155" }}
          >
            <PresetOptGroups presets={presets} />
            {collections.map((c) => <option key={c} value={c}>{c}</option>)}
          </select>
          <div style={{ flex: 1 }} />
          <button
            type="button"
            disabled={!report}
            onClick={() => report && download("audit-trail.csv", batchToCsv(report, models), "text/csv")}
            style={{ ...exportBtn, opacity: report ? 1 : 0.5, cursor: report ? "pointer" : "not-allowed" }}
            data-testid="audit-export-csv"
          >
            Export Audit Trail (CSV)
          </button>
          <button
            type="button"
            disabled={!report}
            onClick={() => report && download("audit-trail.json", JSON.stringify(report, null, 2), "application/json")}
            style={{ ...exportBtn, opacity: report ? 1 : 0.5, cursor: report ? "pointer" : "not-allowed" }}
            data-testid="audit-export-json"
          >
            Export JSON
          </button>
          <InfoButton {...TOOL_HELP.auditHistory} testId="audit-history" />
        </div>
        <HistoryTimeline history={backendHistory} />
      </div>
    </section>
  );
}

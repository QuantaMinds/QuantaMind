import { useEffect, useState } from "react";
import { useEvalRegistryStore, DEFAULT_PRESET } from "../../eval/state/evalRegistryStore";
import { useBatchStore } from "../../eval/state/batchStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useBackendStore } from "../../../shared/state/backendStore";
import { loadCollectionHistory, type LoadedHistory, type RunSummary } from "../../../shared/ipc/eval/matrix";
import { formatIpcError } from "../../../shared/ipc/core/error";
import type { BackendKind } from "../../../shared/ipc/models/storage";
import { HistoryTimeline } from "../../eval/components/matrix/HistoryTimeline";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { CliCommandPreview } from "../../../shared/cli/CliCommandPreview";
import { buildRunHistory } from "../../../shared/cli/qmCommand";
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
const BACKEND_LABEL: Record<BackendKind, string> = { llama_cpp: "llama.cpp", vllm: "vLLM" };
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
  const historyModel = useSelectedModelStore((s) => s.selectedModels[0]?.name ?? null); // for the Run History CLI hint
  const [collection, setCollection] = useState(DEFAULT_PRESET);
  const [history, setHistory] = useState<RunSummary[]>([]);
  // Rows on disk this build can't interpret (e.g. recorded on a backend that no
  // longer exists). Shown as a note beside the graph — a short list must never
  // read as "these are all your runs".
  const [unreadable, setUnreadable] = useState(0);
  // A load failure is surfaced, never swallowed — an empty graph must distinguish
  // "no runs yet" from "the history failed to load" (the two used to look identical).
  const [historyError, setHistoryError] = useState<string | null>(null);
  // Show only the selected backend's regression history — a backend switch
  // shouldn't keep displaying the previous backend's model runs.
  const backendHistory = history.filter((h) => h.backend === selectedBackend);

  useEffect(() => {
    void init().catch((e) => console.error("eval registry init failed (AuditPage):", e));
  }, [init]);

  // A load failure is shown (not swallowed into a misleading empty state); success clears it.
  const applyHistory = (cancelled: () => boolean) => ({
    ok: (h: LoadedHistory) => {
      if (cancelled()) return;
      setHistory(h.entries);
      setUnreadable(h.unreadable);
      setHistoryError(null);
    },
    fail: (e: unknown) => {
      if (cancelled()) return;
      setHistory([]);
      setUnreadable(0);
      setHistoryError(formatIpcError(e));
    },
  });

  useEffect(() => {
    let cancelled = false;
    const h = applyHistory(() => cancelled);
    loadCollectionHistory(collection).then(h.ok).catch(h.fail);
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [collection]);

  // Re-read the on-disk history whenever a batch finishes FOR THE SHOWN collection.
  // The backend appends to the history file before emitting `batch-complete`, so a
  // re-fetch here picks up the new run without an app restart. Guarding on the
  // collection id keeps a run for a different collection from clobbering the graph.
  useEffect(() => {
    if (!report || report.collection_id !== collection) return;
    let cancelled = false;
    const h = applyHistory(() => cancelled);
    loadCollectionHistory(collection).then(h.ok).catch(h.fail);
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, collection]);

  return (
    <section data-testid="tab-audit" className="space-y-4">
      {/* The Context-Cliff probe sits on top; the run history follows below it. */}
      <ContextCliffPanel />

      <h2 className="text-lg font-semibold text-slate-900">Run History</h2>

      <div style={card} data-testid="audit-history">
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
          <span style={{ fontSize: 13, fontWeight: 650, color: "#1e293b", fontFamily: "Inter,sans-serif" }}>Results History</span>
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
            onClick={() => report && download("run-metrics.csv", batchToCsv(report, models), "text/csv")}
            style={{ ...exportBtn, opacity: report ? 1 : 0.5, cursor: report ? "pointer" : "not-allowed" }}
            data-testid="audit-export-csv"
          >
            Export Metrics (CSV)
          </button>
          <button
            type="button"
            disabled={!report}
            onClick={() => report && download("run-metrics.json", JSON.stringify(report, null, 2), "application/json")}
            style={{ ...exportBtn, opacity: report ? 1 : 0.5, cursor: report ? "pointer" : "not-allowed" }}
            data-testid="audit-export-json"
          >
            Export JSON
          </button>
          <InfoButton {...TOOL_HELP.auditHistory} testId="audit-history" />
        </div>
        <div style={{ marginBottom: 10 }}>
          <CliCommandPreview
            testId="run-history-cli-preview"
            cmd={buildRunHistory({
              backend: selectedBackend,
              model: historyModel,
              collection,
              isCustom: !presets.some((p) => p.id === collection),
              profile: "general-agent",
            })}
          />
        </div>
        {historyError ? (
          // A real load failure — never let it masquerade as the empty "no runs yet" state.
          <p
            data-testid="audit-history-error"
            style={{
              fontSize: 12, color: "#b91c1c", fontFamily: "Inter, sans-serif",
              background: "#fef2f2", border: "1px solid #fee2e2", borderRadius: 6, padding: "10px 12px", margin: 0,
            }}
          >
            Couldn't load run history — {historyError}
          </p>
        ) : history.length > 0 && backendHistory.length === 0 ? (
          // Runs exist, just not for the selected backend — say so instead of "no runs yet".
          <p
            data-testid="audit-history-other-backend"
            style={{ padding: 30, textAlign: "center", color: "#475569", fontSize: 13, fontFamily: "Inter, sans-serif" }}
          >
            No runs for {BACKEND_LABEL[selectedBackend]} yet — {history.length} run{history.length === 1 ? "" : "s"} recorded under other backends.
          </p>
        ) : (
          // Genuinely no runs (HistoryTimeline renders its own "No run history yet" empty state).
          <HistoryTimeline history={backendHistory} />
        )}
        {!historyError && unreadable > 0 && (
          // Loud about what ISN'T shown: skipping these silently would make a
          // truncated timeline read as the complete record.
          <p
            data-testid="audit-history-unreadable"
            style={{
              fontSize: 11, color: "#92400e", fontFamily: "Inter, sans-serif",
              background: "#fffbeb", border: "1px solid #fde68a", borderRadius: 6,
              padding: "8px 10px", margin: "8px 0 0",
            }}
          >
            {unreadable} older run{unreadable === 1 ? "" : "s"} couldn't be read and {unreadable === 1 ? "is" : "are"} not
            shown — {unreadable === 1 ? "it was" : "they were"} recorded on a backend this version no longer supports.
            The {unreadable === 1 ? "record is" : "records are"} still on disk, untouched.
          </p>
        )}
      </div>
    </section>
  );
}

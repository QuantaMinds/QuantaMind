import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { readTextCapped } from "../../../../shared/ipc/eval/registry";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { useNavStore } from "../../../../shared/state/navStore";
import type { ToolTask } from "../../../../shared/ipc/eval/registry";
import { csvToCollection, EXPECTED_HEADER } from "../../csvImport";

const TOOLS_TEMPLATE = JSON.stringify(
  [
    {
      name: "get_weather",
      description: "Get the current weather for a city",
      parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
    },
  ],
  null,
  2,
);

const EXAMPLE_ROWS = [
  { id: "weather-paris", prompt: "What's the weather in Paris?", tool: "get_weather", args: '{"city":"Paris"}' },
  { id: "refuse-greet", prompt: "Say hello", tool: "(empty → abstain)", args: "(empty)" },
];

const RAW_EXAMPLE =
  `${EXPECTED_HEADER.join(",")}\n` +
  `weather-paris,"What's the weather in Paris?",get_weather,"{""city"":""Paris""}"\n` +
  `refuse-greet,"Say hello",,`;

function stemOf(path: string): string {
  return (path.split(/[\\/]/).pop() ?? "").replace(/\.csv$/i, "");
}

/// The rich CSV-import modal: teaches the strict format (example table + raw
/// block), takes a shared Tools schema + a pasted/picked CSV + a collection name,
/// validates live with per-row ✓/✗, and only enables Import when fully clean.
export function CsvImportModal({
  onImport,
  onClose,
}: {
  onImport: (name: string, tasks: ToolTask[]) => Promise<void>;
  onClose: () => void;
}) {
  const goDocs = useNavStore((s) => s.setTopView);
  const [toolsJson, setToolsJson] = useState(TOOLS_TEMPLATE);
  const [csvText, setCsvText] = useState("");
  const [name, setName] = useState("");
  const [fileError, setFileError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const result = useMemo(() => csvToCollection(csvText, toolsJson), [csvText, toolsJson]);
  const validCount = result.rows.filter((r) => r.ok).length;
  const errorCount = result.rows.length - validCount;
  const canImport = !!result.tasks && !!name.trim() && !busy;

  const pickFile = async () => {
    setFileError(null);
    try {
      const picked = await open({ multiple: false, filters: [{ name: "CSV", extensions: ["csv"] }] });
      // open() returns a path string (or, in some plugin versions, { path }); null = cancelled.
      const path = typeof picked === "string" ? picked : (picked as { path?: string } | null)?.path;
      if (!path) return; // user cancelled
      const text = await readTextCapped(path);
      if (!text.trim()) {
        setFileError("The selected file is empty.");
        return;
      }
      setCsvText(text);
      if (!name.trim()) setName(stemOf(path));
    } catch (e) {
      setFileError(formatIpcError(e));
    }
  };

  const submit = async () => {
    if (!result.tasks || !name.trim()) return;
    setBusy(true);
    try {
      await onImport(name.trim(), result.tasks);
      onClose();
    } catch (e) {
      setFileError(formatIpcError(e));
      setBusy(false);
    }
  };

  // Deep-link into the merged Docs tab: the Tests reference page, scrolled to the CSV-import block
  // (the block heading's slug — see features/docs/reference.ts).
  const learnMore = () => { goDocs("docs"); setTimeout(() => { location.hash = "#docs-reference-eval--collection-editor-csv-import"; }, 0); };

  return (
    <div
      role="presentation"
      onClick={onClose}
      className="fixed inset-0 z-40 bg-black/50 flex items-center justify-center p-4"
      data-testid="csv-import-modal"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Import tasks from CSV"
        onClick={(e) => e.stopPropagation()}
        className="bg-surface rounded-xl shadow-2xl w-[760px] max-w-[94vw] max-h-[88vh] overflow-y-auto p-6 space-y-5 border border-gray-200"
      >
        {/* Title */}
        <div>
          <h2 className="text-base font-bold text-gray-900">Import tasks from CSV</h2>
          <p className="text-xs text-gray-500 mt-1">
            Bulk-load single-turn tool-call tasks from a spreadsheet. Tool schemas are defined once below and
            apply to every row. Parallel / agentic tasks → use <span className="text-gray-700 font-semibold">Import .json</span>.
          </p>
        </div>

        {/* Format guide */}
        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-wide text-gray-500">Expected format</div>
          <div className="rounded-lg border border-gray-200 overflow-hidden">
            <table className="w-full text-xs" data-testid="csv-import-example">
              <thead>
                <tr className="bg-gray-50 text-gray-500">
                  {EXPECTED_HEADER.map((h) => (
                    <th key={h} className="text-left font-semibold px-3 py-2 font-mono">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody className="font-mono text-gray-700">
                {EXAMPLE_ROWS.map((r) => (
                  <tr key={r.id} className="border-t border-gray-100">
                    <td className="px-3 py-1.5">{r.id}</td>
                    <td className="px-3 py-1.5">{r.prompt}</td>
                    <td className="px-3 py-1.5 text-gray-500">{r.tool}</td>
                    <td className="px-3 py-1.5 text-gray-500">{r.args}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <pre className="text-[11px] bg-gray-50 border border-gray-200 rounded-md p-3 text-gray-600 overflow-x-auto font-mono">{RAW_EXAMPLE}</pre>
          <p className="text-[11px] text-gray-500">
            Empty <span className="font-mono text-gray-700">expected_tool</span> = an abstain task (the model should
            make no tool call). <button type="button" onClick={learnMore} data-testid="csv-import-learnmore" className="text-blue-600 hover:text-blue-700 underline underline-offset-2 transition-colors">Learn more ↗</button>
          </p>
        </div>

        {/* Tools schema */}
        <Field label="Tools schema (JSON) — applies to every row">
          <textarea
            value={toolsJson}
            onChange={(e) => setToolsJson(e.target.value)}
            rows={6}
            data-testid="csv-import-tools"
            className="w-full rounded-md bg-white border border-gray-300 focus:border-blue-600 focus:ring-1 focus:ring-blue-600 px-3 py-2 text-xs text-gray-900 font-mono outline-none resize-y transition-colors"
          />
          {result.toolsError && (
            <div className="text-xs text-red-500 mt-1 font-semibold" data-testid="csv-import-tools-error">{result.toolsError}</div>
          )}
        </Field>

        {/* CSV input */}
        <Field label="CSV data">
          <div className="flex items-center gap-2">
            <button type="button" onClick={() => void pickFile()} data-testid="csv-import-file" className="px-3 py-1.5 rounded-md text-xs text-gray-700 bg-white border border-gray-300 hover:bg-gray-50 transition-colors">
              Choose .csv file…
            </button>
            <span className="text-[11px] text-gray-500">or paste below</span>
          </div>
          <textarea
            value={csvText}
            onChange={(e) => setCsvText(e.target.value)}
            rows={6}
            placeholder={RAW_EXAMPLE}
            data-testid="csv-import-paste"
            className="w-full rounded-md bg-white border border-gray-300 focus:border-blue-600 focus:ring-1 focus:ring-blue-600 px-3 py-2 text-xs text-gray-900 font-mono outline-none resize-y transition-colors"
          />
        </Field>

        {/* Collection name */}
        <Field label="Collection name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-test-cases"
            data-testid="csv-import-name"
            className="w-full rounded-md bg-white border border-gray-300 focus:border-blue-600 focus:ring-1 focus:ring-blue-600 px-3 py-2 text-sm text-gray-900 outline-none transition-colors"
          />
        </Field>

        {/* Live preview / validation — only once the user has supplied input, so an
            untouched modal never shows a spurious "CSV is empty" error. */}
        {csvText.trim() && (
          <div className="space-y-2" data-testid="csv-import-preview">
            {result.headerError && (
              <div className="text-xs text-red-600 bg-red-50 border border-red-200 rounded-md px-3 py-2" data-testid="csv-import-header-error">
                Header error — {result.headerError}
              </div>
            )}
            {!result.headerError && result.rows.length === 0 && (
              <div className="text-xs text-amber-600 bg-amber-50 border border-amber-200 rounded-md px-3 py-2" data-testid="csv-import-norows">
                No data rows found — add at least one task row below the header.
              </div>
            )}
            {!result.headerError && result.rows.length > 0 && (
              <>
                <div className="text-[11px] text-gray-500">
                  <span className="text-green-600 font-semibold">{validCount} valid</span>
                  {errorCount > 0 && <span className="text-red-500 font-semibold"> · {errorCount} error{errorCount > 1 ? "s" : ""}</span>}
                </div>
                <div className="max-h-44 overflow-y-auto rounded-md border border-gray-200 divide-y divide-gray-100">
                  {result.rows.map((r) => (
                    <div key={r.row} className="flex items-start gap-2 px-3 py-1.5 text-xs" data-testid={`csv-row-${r.ok ? "ok" : "err"}-${r.row}`}>
                      <span className={r.ok ? "text-green-500 font-bold" : "text-red-500 font-bold"}>{r.ok ? "✓" : "✗"}</span>
                      <span className="text-gray-500 font-mono font-medium">Row {r.row}{r.id ? ` (${r.id})` : ""}</span>
                      {!r.ok && <span className="text-red-500 font-semibold">— {r.message}</span>}
                    </div>
                  ))}
                </div>
              </>
            )}
          </div>
        )}

        {fileError && <div className="text-xs text-red-500 mt-1 font-semibold" data-testid="csv-import-file-error">{fileError}</div>}

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 pt-1">
          <button type="button" onClick={onClose} data-testid="csv-import-cancel" className="px-3 py-1.5 rounded-md text-sm text-gray-500 hover:bg-gray-100 transition-colors">
            Cancel
          </button>
          <button
            type="button"
            disabled={!canImport}
            onClick={() => void submit()}
            data-testid="csv-import-submit"
            className="px-4 py-1.5 rounded-md text-sm text-white bg-blue-600 hover:bg-blue-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            Import {result.tasks ? `${result.tasks.length} task${result.tasks.length > 1 ? "s" : ""}` : ""}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <div className="text-[11px] font-bold uppercase tracking-wide text-gray-500">{label}</div>
      {children}
    </div>
  );
}

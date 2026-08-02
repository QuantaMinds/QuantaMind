import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { saveCompareReport } from "../../../shared/ipc/compare/compare";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { useCompareStore } from "../state/compareStore";
import { useSelectedModelStore } from "../../../shared/state/selectedModelStore";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { buildReport } from "../format/buildReport";
import { toMarkdown } from "../format/markdownReport";
import { toJson } from "../format/jsonReport";

const FORMAT_LABEL: Record<"md" | "json", string> = { md: "MD", json: "JSON" };

export function ExportButtons() {
  const [error, setError] = useState<string | null>(null);
  const rows = useCompareStore((s) => s.rows);
  const disabled = rows.length === 0;

  const exportAs = async (format: "md" | "json") => {
    setError(null);
    const path = await save({
      defaultPath: `quantamind-compare-${Date.now()}.${format}`,
      filters: [{ name: FORMAT_LABEL[format], extensions: [format] }],
    });
    if (!path) return;
    const s = useCompareStore.getState();
    const report = buildReport({
      prompt: s.prompt, systemPrompt: s.systemPrompt,
      hardwareSnapshot: s.hardwareSnapshot,
      selectedModels: useSelectedModelStore.getState().selectedModels, rows: s.rows,
      installed: useInstalledModelsStore.getState().list,
    });
    const contents = format === "md" ? toMarkdown(report) : toJson(report);
    try {
      await saveCompareReport(path, format, contents);
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  return (
    <div className="flex items-center gap-2" data-testid="export-buttons">
      <button
        type="button" disabled={disabled} onClick={() => void exportAs("md")}
        className="text-sm border rounded px-3 py-1 disabled:opacity-50"
        data-testid="export-md"
      >
        Export Markdown
      </button>
      <button
        type="button" disabled={disabled} onClick={() => void exportAs("json")}
        className="text-sm border rounded px-3 py-1 disabled:opacity-50"
        data-testid="export-json"
      >
        Export JSON
      </button>
      {error && (
        <span role="alert" className="text-xs text-red-600" data-testid="export-error">
          {error}
        </span>
      )}
    </div>
  );
}

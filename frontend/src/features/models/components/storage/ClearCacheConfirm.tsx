import { useState } from "react";

const CONFIRM_WORD = "CLEAR";

/// Type-to-confirm guard for wiping regenerable app caches. The destructive
/// button stays disabled until the user types CLEAR, so a bulk delete can't
/// happen on a single misclick. By default custom collections, settings, and
/// models are kept; an opt-in checkbox additionally clears the re-downloadable
/// HuggingFace model snapshot cache. The copy spells out exactly what each does.
export function ClearCacheConfirm({
  onConfirm,
  onCancel,
  busy,
  error,
}: {
  onConfirm: (includeModels: boolean) => void;
  onCancel: () => void;
  busy: boolean;
  error: string | null;
}) {
  const [typed, setTyped] = useState("");
  const [includeModels, setIncludeModels] = useState(false);
  const armed = typed.trim().toUpperCase() === CONFIRM_WORD;

  return (
    <div role="alertdialog" data-testid="clear-cache-confirm" className="border rounded p-3 bg-amber-50 text-xs">
      Clear cached app data? This deletes eval history, batch reports, job logs,
      pipeline traces, context stress test measurements, and the recent-workspace list.
      <strong> Custom eval collections, readiness profiles, and settings are kept.</strong>
      <label className="flex items-start gap-2 mt-2">
        <input
          type="checkbox"
          checked={includeModels}
          onChange={(e) => setIncludeModels(e.target.checked)}
          disabled={busy}
          data-testid="clear-cache-models"
          className="mt-0.5"
        />
        <span>
          Also clear the HuggingFace model cache (MLX/whisper snapshots). These
          re-download on next use; your app-managed models in <code>~/.quantamind</code>{" "}
          are not affected.
        </span>
      </label>
      <label className="flex flex-col gap-1 mt-2">
        Type <strong>{CONFIRM_WORD}</strong> to confirm:
        <input
          type="text"
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          disabled={busy}
          data-testid="clear-cache-input"
          className="border rounded px-2 py-1 w-40"
          autoFocus
        />
      </label>
      {error && <div role="alert" className="text-red-600 mt-2">{error}</div>}
      <div className="flex gap-2 mt-2">
        <button
          type="button"
          onClick={() => onConfirm(includeModels)}
          disabled={!armed || busy}
          data-testid="clear-cache-confirm-btn"
          className="border rounded px-2 py-1 bg-red-600 text-white disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {busy ? "Clearing…" : "Clear cache"}
        </button>
        <button type="button" onClick={onCancel} disabled={busy} className="border rounded px-2 py-1">
          Cancel
        </button>
      </div>
    </div>
  );
}

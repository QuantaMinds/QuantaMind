import { useEffect, useState } from "react";
import { formatIpcError } from "../../shared/ipc/core/error";
import { previewPublishPayload, type PublishPreview } from "../../shared/ipc/publish/preview";
import type { ModelVerdict } from "../../shared/ipc/eval/readiness";
import type { InferenceParams } from "../../shared/ipc/workspace/prompts";
import { WhatsSharedPanel } from "./WhatsSharedPanel";
import { isAllowedWriteupLink } from "./writeupLink";

interface Props {
  verdicts: ModelVerdict[];
  /// The active collection id — the backend stamps its identity/name on each row.
  collectionId: string;
  /// The run's content-verified leaderboard hash (from the report): a string only for a pristine
  /// bundled collection, null for custom/imported OR any edit. The backend uses THIS to exclude
  /// non-publishable rows (never re-derives from the id) — so an edited collection can't publish.
  collectionHash: string | null;
  /// The inference params the RUN actually used, stamped on the batch report at run time (like
  /// `collection_hash`). Never the live global header — editing the header after a run must not
  /// change what's published. `{}` = the run sent none (backend defaults) / older report.
  params: InferenceParams;
  onClose: () => void;
  /// Invoked with the agreed preview, the (optional, allow-listed) write-up link, and
  /// the exact params snapshot the preview was built from — so what's published is
  /// byte-identical to what the user opted into (no re-read race if the header changes).
  onPublish: (preview: PublishPreview, link: string, params: InferenceParams) => void;
}

/// The privacy gate: build the exact payload preview in Rust, show the user what
/// will (and won't) leave their machine plus the raw JSON, and require an explicit
/// default-OFF opt-in before Publish enables. Aggregate-only, community-reported.
export function PublishDialog({ verdicts, collectionId, collectionHash, params, onClose, onPublish }: Props) {
  const [preview, setPreview] = useState<PublishPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [agreed, setAgreed] = useState(false);
  const [link, setLink] = useState("");
  const linkOk = isAllowedWriteupLink(link);

  useEffect(() => {
    let live = true;
    previewPublishPayload(verdicts, params, collectionId, collectionHash)
      .then((p) => live && setPreview(p))
      .catch((e) => live && setError(formatIpcError(e)));
    return () => {
      live = false;
    };
  }, [verdicts, params, collectionId, collectionHash]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const canPublish = !!preview && preview.rows.length > 0 && !preview.invalid && agreed && linkOk;

  /// Why Publish is disabled — surfaced as a button tooltip so a greyed-out button is
  /// never a dead end the user has to guess at.
  const disabledReason = !preview
    ? "Building the payload preview…"
    : preview.invalid
      ? `Row ${preview.invalid.index} failed validation`
      : preview.rows.length === 0
        ? "No measured results to publish yet"
        : !agreed
          ? "Tick the opt-in box to publish"
          : !linkOk
            ? "Write-up link isn't on the allow-list"
            : "";

  return (
    <div role="presentation" onClick={onClose} data-testid="publish-dialog" className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center p-4">
      <div role="dialog" aria-modal="true" aria-label="Publish to community board" onClick={(e) => e.stopPropagation()} className="bg-white rounded-xl shadow-2xl w-[34rem] max-w-[94vw] p-5 space-y-4 border border-slate-200 max-h-[90vh] overflow-y-auto">
        <div className="flex items-center justify-between border-b border-slate-100 pb-3">
          <h3 className="text-sm font-bold text-slate-900">Publish to community board</h3>
          <span className="text-[11px] text-slate-400">aggregate-only · community-reported · never sold</span>
        </div>

        {error && <div data-testid="publish-error" className="text-xs text-red-600">{error}</div>}

        {preview && preview.rows.length === 0 && !preview.invalid && (
          <div data-testid="publish-empty" className="text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded p-3 space-y-1">
            <div className="font-semibold">Nothing to publish yet</div>
            <p>
              A result is only publishable once it has a <b>measured Pass^k</b> (run an evaluation), a{" "}
              <b>known quantization</b>, and a <b>built-in collection</b> — results on your own custom
              collections stay local and are never published.
              {preview.excluded_count > 0 && <> All {preview.excluded_count} model{preview.excluded_count === 1 ? " was" : "s were"} excluded for missing one of these.</>}
            </p>
            <p>Run an eval on at least one quantized model with a built-in collection, then reopen this dialog.</p>
          </div>
        )}

        {preview && preview.rows.length > 0 && (
          <>
            <WhatsSharedPanel />
            <p className="text-xs text-slate-500">
              {/* One row PER MEASURED PATH — a model evaluated on both native + prompt-based
                  publishes two results, so this counts results (rows), not distinct models. */}
              Publishing <b>{preview.rows.length}</b> result{preview.rows.length === 1 ? "" : "s"} in cohort{" "}
              <code className="text-slate-700">{preview.cohort_key}</code>
              {preview.excluded_count > 0 && <> · {preview.excluded_count} excluded (no measured Pass^k)</>}.
            </p>
            {preview.invalid && (
              <div data-testid="publish-invalid" className="text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded p-2">
                Row {preview.invalid.index}: {preview.invalid.reason}
              </div>
            )}
            <div>
              <div className="text-[11px] font-semibold text-slate-500 uppercase tracking-wide mb-1">Exact payload</div>
              <pre data-testid="publish-raw-payload" className="text-[11px] bg-slate-50 border border-slate-200 rounded p-2 max-h-40 overflow-auto text-slate-700 whitespace-pre-wrap break-all">
                {preview.canonical_json}
              </pre>
            </div>
            <label className="flex flex-col gap-1 text-xs text-slate-700">
              <span className="font-semibold">Link to your write-up <span className="font-normal text-slate-400">(optional)</span></span>
              <input
                type="url"
                data-testid="publish-link"
                value={link}
                onChange={(e) => setLink(e.target.value)}
                placeholder="https://github.com/… · x.com · dev.to · reddit.com · …"
                className={`bg-white border rounded-md px-2 py-1 text-slate-900 outline-none ${linkOk ? "border-slate-300 focus:border-slate-400" : "border-red-400"}`}
              />
              {!linkOk && <span data-testid="publish-link-hint" className="text-red-600">Only https links to github.com, x.com, dev.to, reddit.com, medium.com, youtube.com, or huggingface.co are allowed.</span>}
            </label>
            <label className="flex items-start gap-2 text-xs text-slate-700 cursor-pointer">
              <input type="checkbox" data-testid="publish-optin" checked={agreed} onChange={(e) => setAgreed(e.target.checked)} className="mt-0.5 cursor-pointer" />
              <span>I understand exactly what's shared above and opt in to publishing it.</span>
            </label>
          </>
        )}

        {!preview && !error && <div className="text-xs text-slate-400 py-6 text-center">Building payload preview…</div>}

        <div className="flex justify-end gap-2 pt-2 border-t border-slate-100">
          <button type="button" onClick={onClose} data-testid="publish-cancel" className="px-3 py-1.5 rounded-md text-sm text-slate-600 hover:bg-slate-100 transition-colors">
            Cancel
          </button>
          <button type="button" disabled={!canPublish} title={disabledReason} data-testid="publish-confirm" onClick={() => preview && onPublish(preview, link.trim(), params)} className="px-3 py-1.5 rounded-md text-sm font-semibold text-white bg-slate-900 hover:bg-slate-800 disabled:opacity-50">
            Publish
          </button>
        </div>
      </div>
    </div>
  );
}

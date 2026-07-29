import { useState } from "react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useToast } from "../../shared/ui/Toast";
import { formatIpcError } from "../../shared/ipc/core/error";
import { publishToBoard, startLogin } from "../../shared/ipc/publish/publish";
import type { PublishPreview } from "../../shared/ipc/publish/preview";
import type { ModelVerdict } from "../../shared/ipc/eval/readiness";
import type { InferenceParams } from "../../shared/ipc/workspace/prompts";
import { PublishDialog } from "./PublishDialog";

/// Opens the privacy-gate dialog and handles every publish outcome WITHOUT ever
/// freezing the UI — each server status becomes a toast/next-action. The offline
/// app keeps working regardless of what the board returns.
export function PublishButton({ verdicts, collectionId, collectionHash, runParams }: { verdicts: ModelVerdict[]; collectionId: string; collectionHash: string | null; runParams: InferenceParams }) {
  const toast = useToast();
  const [open, setOpen] = useState(false);

  /// `retried` guards the auto-publish-after-login path: on first `needs_auth` we
  /// open sign-in then re-attempt once. If it still needs auth (sign-in cancelled
  /// or failed), we stop and ask the user instead of looping forever.
  const onPublish = async (preview: PublishPreview, link: string, params: InferenceParams, retried = false) => {
    try {
      const outcome = await publishToBoard(verdicts, params, collectionId, collectionHash, link);
      switch (outcome.kind) {
        case "ok":
          toast("Published to the board ✓");
          setOpen(false);
          void openUrl(outcome.board_url).catch(() => {});
          break;
        case "needs_auth": {
          if (retried) {
            toast("Sign-in didn't complete — Publish again when ready");
            break;
          }
          toast("Sign in in your browser to publish…");
          let persisted: boolean;
          try {
            persisted = await startLogin();
          } catch (e) {
            toast(formatIpcError(e));
            break;
          }
          if (!persisted) {
            toast("Signed in for this session — Keychain access was denied, so you may need to sign in again next launch");
          }
          await onPublish(preview, link, params, true);
          break;
        }
        case "invalid":
          toast(`Row ${outcome.index} was rejected — adjust it and retry`);
          break;
        case "rate_limited":
          toast("Publishing a lot — try again shortly");
          break;
        case "update_required":
          toast("Please update QuantaMind to publish");
          break;
      }
    } catch (e) {
      toast(`Publish failed — ${formatIpcError(e)}`);
    }
  };

  return (
    <>
      <button
        type="button"
        data-testid="publish-open"
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-2 bg-slate-950 hover:bg-slate-900 text-white rounded-lg text-sm font-semibold py-2 px-4 shadow-sm transition-all cursor-pointer"
      >
        🚀 Deploy
      </button>
      {open && <PublishDialog verdicts={verdicts} collectionId={collectionId} collectionHash={collectionHash} params={runParams} onClose={() => setOpen(false)} onPublish={onPublish} />}
    </>
  );
}

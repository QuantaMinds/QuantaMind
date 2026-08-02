import { cancelHfInstall } from "../../../../shared/ipc/models/hf_install";
import type { DownloadEntry } from "../../state/modelStore";

/// Dispatch the right cancel IPC for a download entry by source.
/// Returns null on success, an Error on failure (caller decides
/// whether to keep the entry or surface the failure).
export async function cancelEntry(entry: DownloadEntry): Promise<Error | null> {
  try {
    if (entry.source === "huggingface") {
      await cancelHfInstall();
    }
    return null;
  } catch (e) {
    return e instanceof Error ? e : new Error(String(e));
  }
}

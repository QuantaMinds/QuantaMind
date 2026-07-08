import { useEffect, useState } from "react";
import { getDiskUsage, type DiskUsage } from "../../../../shared/ipc/models/storage";
import { clearAppCache } from "../../../../shared/ipc/cache";
import { formatBytes } from "../../../../shared/format/bytes";
import { StoragePathSection } from "../StoragePathSection";
import { ModelsFolderSection } from "../ModelsFolderSection";
import { ClearCacheConfirm } from "./ClearCacheConfirm";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../../state/installedModelsStore";
import { useBatchStore } from "../../../eval/state/batchStore";
import { useCliffStore } from "../../../eval/state/cliffStore";

/// Storage controls shown at the top of the Downloads page: the Ollama models
/// path, the shared GGUF weights folder, and a disk-usage summary.
export function StorageSection() {
  const list = useInstalledModelsStore((s) => s.list);
  const [usage, setUsage] = useState<DiskUsage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [busy, setBusy] = useState(false);
  const [clearError, setClearError] = useState<string | null>(null);
  const [freed, setFreed] = useState<number | null>(null);

  useEffect(() => {
    getDiskUsage()
      .then((u) => {
        setUsage(u);
        setError(null);
      })
      .catch((e) => setError(formatIpcError(e)));
  }, [list]);

  const onConfirmClear = async (includeModels: boolean) => {
    setBusy(true);
    setClearError(null);
    try {
      const bytes = await clearAppCache(includeModels);
      // The deleted caches back these in-memory views; reset so stale history,
      // reports, and cliff points don't linger after the files are gone.
      useBatchStore.getState().reset();
      useCliffStore.getState().reset();
      setFreed(bytes);
      setPending(false);
    } catch (e) {
      setClearError(formatIpcError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-testid="storage-section" className="flex flex-col gap-3">
      <StoragePathSection />
      <ModelsFolderSection />
      {usage && (
        <div className="text-xs text-gray-600" data-testid="disk-summary">
          Models: {formatBytes(usage.ollama_models_bytes)} / Free:{" "}
          {formatBytes(usage.free_bytes)} on disk
        </div>
      )}
      {error && <div role="alert" className="text-red-600 text-xs">{error}</div>}
      {pending ? (
        <ClearCacheConfirm
          onConfirm={(includeModels) => void onConfirmClear(includeModels)}
          onCancel={() => setPending(false)}
          busy={busy}
          error={clearError}
        />
      ) : (
        <div className="flex items-center gap-2 text-xs">
          <button
            type="button"
            onClick={() => {
              setFreed(null);
              setClearError(null);
              setPending(true);
            }}
            data-testid="clear-cache-button"
            className="border rounded px-2 py-1 self-start"
          >
            Clear cache
          </button>
          {freed !== null && (
            <span className="text-gray-600" data-testid="clear-cache-result">
              Freed {formatBytes(freed)} of cached data.
            </span>
          )}
        </div>
      )}
    </div>
  );
}

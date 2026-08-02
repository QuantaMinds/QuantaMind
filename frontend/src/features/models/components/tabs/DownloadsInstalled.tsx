import { useEffect, useState } from "react";
import { deleteLlamaModel } from "../../../../shared/ipc/models/llama_start";
import { formatBytes } from "../../../../shared/format/bytes";
import { formatIpcError } from "../../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../../state/installedModelsStore";
import { groupInstalled } from "../../state/installedGroups";
import { ConfirmRemove } from "../ConfirmRemove";

const badge = "text-[10px] px-1 py-0.5 rounded";

export function DownloadsInstalled() {
  const list = useInstalledModelsStore((s) => s.list);
  const status = useInstalledModelsStore((s) => s.status);
  const storeError = useInstalledModelsStore((s) => s.error);
  const refresh = useInstalledModelsStore((s) => s.refresh);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);

  useEffect(() => {
    if (status === "idle") void refresh();
  }, [status, refresh]);

  const groups = groupInstalled(list);
  const target = groups.find((g) => g.name === pending);

  const onDelete = async () => {
    if (!target?.llamaPath) return;
    setError(null);
    try {
      await deleteLlamaModel(target.llamaPath);
      await refresh();
      setPending(null);
    } catch (e) {
      setError(formatIpcError(e));
    }
  };

  const showErr = error ?? storeError;
  if (groups.length === 0) {
    return (
      <div className="text-xs text-gray-500" data-testid="downloads-empty-installed">
        No installed models yet. Browse the Hugging Face or Local File tabs.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2" data-testid="downloads-installed-list">
      {showErr && <div role="alert" className="text-red-600 text-xs">{showErr}</div>}
      <ul className="divide-y border rounded">
        {groups.map((g) => (
          <li key={g.name} data-testid={`download-installed-${g.name}`}
            className="px-3 py-2 flex items-center justify-between gap-2">
            <div className="min-w-0">
              <div className="text-sm truncate flex items-center gap-1">
                {g.displayName ?? g.name}
                {g.llamaPath
                  ? <span className={`${badge} bg-amber-50 text-amber-700`}>llama.cpp</span>
                  : <span className={`${badge} bg-slate-100 text-slate-600`}>remote</span>}
              </div>
              <div className="text-[11px] text-gray-500">
                {g.family} · {g.parameterSize} · {g.quantization} · {formatBytes(g.sizeBytes)}
              </div>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              {/* Only a locally-installed GGUF is ours to delete; a model served by a
                  remote vLLM/SGLang box lives on that machine. */}
              {g.llamaPath && (
                <button type="button" onClick={() => setPending(g.name)}
                  className="text-xs border rounded px-2 py-1" aria-label={`Delete ${g.displayName ?? g.name}`}>
                  Delete
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>
      {target && (
        <ConfirmRemove
          name={target.displayName ?? target.name}
          sizeBytes={target.sizeBytes}
          onConfirm={() => void onDelete()}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}

import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getUserSettings,
  resolveModelsFolder,
  setUserSettings,
} from "../../../shared/ipc/settings/userSettings";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../state/installedModelsStore";

/// Shows the shared GGUF weights folder (used by llama.cpp directly and
/// loaded by llama-server) and lets the user point it elsewhere.
export function ModelsFolderSection() {
  const [path, setPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refreshModels = useInstalledModelsStore((s) => s.refresh);

  const load = useCallback(() => {
    resolveModelsFolder().then(setPath).catch((e) => setError(formatIpcError(e)));
  }, []);
  useEffect(load, [load]);

  const change = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    try {
      const current = await getUserSettings();
      await setUserSettings({ ...current, models_folder: picked });
      load();
      void refreshModels();
    } catch (e) {
      setError(formatIpcError(e));
    }
  }, [load, refreshModels]);

  return (
    <div className="flex flex-col gap-1" data-testid="models-folder-section">
      <span className="text-xs font-medium text-gray-700">Model weights folder</span>
      <p className="text-[11px] text-gray-500">
        GGUFs you download (Hugging Face or local file) are kept here for llama.cpp
        and loaded by llama-server when it runs.
      </p>
      <div className="flex items-center gap-2">
        <code data-testid="models-folder-path" className="flex-1 truncate text-xs text-gray-600">
          {path ?? "…"}
        </code>
        <button
          type="button"
          onClick={() => void change()}
          data-testid="models-folder-change"
          className="border rounded px-2 py-1 text-xs hover:bg-gray-50 shrink-0"
        >
          Change…
        </button>
      </div>
      {error && <span role="alert" className="text-red-600 text-xs">{error}</span>}
    </div>
  );
}

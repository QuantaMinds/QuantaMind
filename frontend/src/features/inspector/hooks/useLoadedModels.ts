import { useCallback, useEffect, useState } from "react";
import { loadedModels, type LoadedModel } from "../../../shared/ipc/system/vram";

/// Fetch the currently-loaded models on mount, keyed by name so
/// each Inspector row can look up its VRAM footprint. Errors degrade to an
/// empty map (the IPC wrapper already logs). `refresh` re-reads on demand.
export function useLoadedModels() {
  const [byName, setByName] = useState<Map<string, LoadedModel>>(new Map());
  const refresh = useCallback(async () => {
    const list = await loadedModels();
    setByName(new Map(list.map((m) => [m.name, m])));
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  return { byName, refresh };
}

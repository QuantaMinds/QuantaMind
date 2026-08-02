import { useCallback } from "react";
import { useInstalledModelsStore } from "../state/installedModelsStore";
import { modelLabel } from "../../../shared/models/modelLabel";

/// Resolve a bare model name — which may be the on-disk path used as the
/// wire id — to its friendly label (the HF repo) via the installed-models list.
/// Falls back to the raw name when the model isn't installed (e.g. an old
/// history entry for a removed model).
export function useModelLabel(): (name: string) => string {
  const list = useInstalledModelsStore((s) => s.list);
  return useCallback(
    (name: string) => {
      const m = list.find((x) => x.name === name);
      return m ? modelLabel(m) : name;
    },
    [list],
  );
}

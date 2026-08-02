import { create } from "zustand";
import {
  listVllmModels,
  type InstalledModelInfo,
} from "../../../shared/ipc/models/storage";
import { listLlamaModels } from "../../../shared/ipc/models/llama_start";
import { formatIpcError } from "../../../shared/ipc/core/error";

export type InstalledStatus = "idle" | "loading" | "ready" | "error";

export interface InstalledModelsState {
  list: InstalledModelInfo[];
  status: InstalledStatus;
  error: string | null;
  lastRefreshedAt: number | null;
  refresh: () => Promise<void>;
  setList: (list: InstalledModelInfo[]) => void;
}

/// Single source of truth for the installed-models list. Install hooks
/// proactively call `refresh()` on success so consumers see the new model even
/// if the backend's `models-changed` broadcast event is dropped (a
/// listener-registration race). The centralized models-changed bus (see
/// installedModelsBus.ts) also drives this same `refresh()`.
export const useInstalledModelsStore = create<InstalledModelsState>(
  (set, get) => ({
    list: [],
    status: "idle",
    error: null,
    lastRefreshedAt: null,
    setList: (list) =>
      set({ list, status: "ready", error: null, lastRefreshedAt: Date.now() }),
    // Fetch each source independently so one still lists when another is down;
    // error only when the LOCAL source fails. vLLM yields [] when its
    // remote endpoint isn't configured/reachable, so those never trip the error
    // path — an unconfigured remote is not a failure.
    refresh: async () => {
      if (get().status === "loading") return;
      set({ status: "loading", error: null });
      const [llama, vllm] = await Promise.allSettled([
        listLlamaModels(),
        listVllmModels(),
      ]);
      if (llama.status === "rejected") {
        set({ status: "error", error: formatIpcError(llama.reason) });
        return;
      }
      const list: InstalledModelInfo[] = [...llama.value];
      if (vllm.status === "fulfilled") list.push(...vllm.value);
      set({ list, status: "ready", error: null, lastRefreshedAt: Date.now() });
    },
  }),
);

import { create } from "zustand";
import {
  getInstalledModelsWithStats,
  listVllmModels,
  listSglangModels,
  type InstalledModelInfo,
} from "../../../shared/ipc/models/storage";
import { listLlamaModels } from "../../../shared/ipc/models/llama_start";
import { listMlxModels } from "../../../shared/ipc/models/mlx";
import { listInstalledSttModels, type InstalledSttModel } from "../../../shared/ipc/stt/stt";
import { formatIpcError } from "../../../shared/ipc/core/error";

export type InstalledStatus = "idle" | "loading" | "ready" | "error";

export interface InstalledModelsState {
  list: InstalledModelInfo[];
  /// Installed STT models (whisper.cpp) — a separate axis from the LLM list, so
  /// they're not forced into the BackendKind-typed list.
  sttList: InstalledSttModel[];
  status: InstalledStatus;
  error: string | null;
  lastRefreshedAt: number | null;
  refresh: () => Promise<void>;
  setList: (list: InstalledModelInfo[]) => void;
}

/// Single source of truth for the installed-models list. Install hooks
/// proactively call `refresh()` on success so consumers see the new
/// model even if the backend's `models-changed` broadcast event is
/// dropped (listener-registration race, /api/tags lag, etc.). The
/// centralized models-changed bus (see installedModelsBus.ts) also
/// drives this same `refresh()`.
export const useInstalledModelsStore = create<InstalledModelsState>(
  (set, get) => ({
    list: [],
    sttList: [],
    status: "idle",
    error: null,
    lastRefreshedAt: null,
    setList: (list) =>
      set({ list, status: "ready", error: null, lastRefreshedAt: Date.now() }),
    // Fetch each backend independently so one still lists when another is
    // down; error only when every source fails. MLX yields [] off Apple
    // Silicon or with no server running, and vLLM/SGLang yield [] when their
    // remote endpoint isn't configured/reachable, so those never trip the error
    // path.
    refresh: async () => {
      if (get().status === "loading") return;
      set({ status: "loading", error: null });
      const [ollama, llama, mlx, vllm, sglang, stt] = await Promise.allSettled([
        getInstalledModelsWithStats(),
        listLlamaModels(),
        listMlxModels(),
        listVllmModels(),
        listSglangModels(),
        listInstalledSttModels(),
      ]);
      const list: InstalledModelInfo[] = [];
      if (ollama.status === "fulfilled") list.push(...ollama.value);
      if (llama.status === "fulfilled") list.push(...llama.value);
      if (mlx.status === "fulfilled") list.push(...mlx.value);
      if (vllm.status === "fulfilled") list.push(...vllm.value);
      if (sglang.status === "fulfilled") list.push(...sglang.value);
      const sttList = stt.status === "fulfilled" ? stt.value : [];
      if (ollama.status === "rejected" && llama.status === "rejected" && mlx.status === "rejected") {
        set({ status: "error", error: formatIpcError(ollama.reason) });
        return;
      }
      set({ list, sttList, status: "ready", error: null, lastRefreshedAt: Date.now() });
    },
  }),
);

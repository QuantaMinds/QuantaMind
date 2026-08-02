import { create } from "zustand";
import { z } from "zod";
import type { RepoKind } from "../../../shared/ipc/models/hf_browse";

// Narrowed to the three Add-Model sub-tabs now that AddModelModal is
// gone (M.5.81). Downloads and Storage are top-level tabs via
// `navStore.topView`, not sub-tabs of the Models page.
export const TabIdSchema = z.enum(["ollama", "huggingface", "local"]);
export type TabId = z.infer<typeof TabIdSchema>;

export type DownloadStatus =
  | "downloading"
  | "installing"
  | "success"
  | "error"
  | "cancelled";
export type DownloadSource = "ollama" | "huggingface" | "local";

export interface DownloadEntry {
  id: string;
  source: DownloadSource;
  name: string;
  status: DownloadStatus;
  percent: number;
  bytesCompleted?: number;
  bytesTotal?: number;
  error?: string | null;
  pullId?: string;
  phaseLabel?: string;
}

export interface ModelStore {
  activeTab: TabId;
  pendingLocalPath: string | null;
  downloads: Record<string, DownloadEntry>;
  activeHfName: string | null;
  activeLocalName: string | null;
  pullNames: Record<string, string>;
  hfSearchQuery: string;
  hfSelectedRepo: string | null;
  // Tags of the selected hit — used to route the detail by the repo's actual
  // format (mlx-tagged → MLX action) rather than the search toggle.
  hfSelectedTags: string[];
  hfRepoKind: RepoKind;
  setActiveTab: (t: TabId) => void;
  setPendingLocalPath: (p: string | null) => void;
  upsertDownload: (entry: DownloadEntry) => void;
  removeDownload: (id: string) => void;
  setActiveHfName: (n: string | null) => void;
  setActiveLocalName: (n: string | null) => void;
  recordPullName: (pullId: string, name: string) => void;
  removePullName: (pullId: string) => void;
  setHfSearchQuery: (q: string) => void;
  setHfSelectedRepo: (repo: string | null, tags?: string[]) => void;
  setHfRepoKind: (k: RepoKind) => void;
}

export const useModelStore = create<ModelStore>((set) => ({
  activeTab: "ollama",
  pendingLocalPath: null,
  downloads: {},
  activeHfName: null,
  activeLocalName: null,
  pullNames: {},
  hfSearchQuery: "",
  hfSelectedRepo: null,
  hfSelectedTags: [],
  hfRepoKind: "gguf",
  setActiveTab: (t) => set({ activeTab: t }),
  setPendingLocalPath: (p) => set({ pendingLocalPath: p }),
  upsertDownload: (entry) =>
    set((s) => ({ downloads: { ...s.downloads, [entry.id]: entry } })),
  removeDownload: (id) =>
    set((s) => {
      const next = { ...s.downloads };
      delete next[id];
      return { downloads: next };
    }),
  setActiveHfName: (n) => set({ activeHfName: n }),
  setActiveLocalName: (n) => set({ activeLocalName: n }),
  recordPullName: (pullId, name) =>
    set((s) => ({ pullNames: { ...s.pullNames, [pullId]: name } })),
  removePullName: (pullId) =>
    set((s) => {
      const next = { ...s.pullNames };
      delete next[pullId];
      return { pullNames: next };
    }),
  setHfSearchQuery: (q) => set({ hfSearchQuery: q }),
  setHfSelectedRepo: (repo, tags = []) =>
    set({ hfSelectedRepo: repo, hfSelectedTags: repo ? tags : [] }),
  // Switching kind drops any open repo detail — a GGUF repo's detail makes no
  // sense under MLX and vice versa.
  setHfRepoKind: (k) => set({ hfRepoKind: k, hfSelectedRepo: null, hfSelectedTags: [] }),
}));

/// Pick the first download entry that's actively in flight, if any.
/// Used by any "one summary line" surface (the page footer, status bar,
/// etc.); replaces the legacy `installInFlight` slot.
export function findActiveDownload(
  downloads: Record<string, DownloadEntry>,
): DownloadEntry | undefined {
  for (const d of Object.values(downloads)) {
    if (d.status === "downloading" || d.status === "installing") return d;
  }
  return undefined;
}

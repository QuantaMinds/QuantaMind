import { create } from "zustand";
import { localServerRss } from "../../../shared/ipc/system/process_memory";
import type { LeakSample } from "../format/leak";

// Session-only series of local-server process RSS tagged by the model that just ran,
// sampled at each run completion. Not persisted — the heuristic only spans the
// app's lifetime, and the model tag lets it ignore model-switch RSS jumps.
interface LeakStore {
  series: LeakSample[];
  sample: (model: string) => Promise<void>;
  reset: () => void;
}

export const useLeakStore = create<LeakStore>((set) => ({
  series: [],
  sample: async (model) => {
    const rss = await localServerRss();
    if (rss == null) return;
    set((s) => ({ series: [...s.series, { model, rssBytes: rss }].slice(-30) }));
  },
  reset: () => set({ series: [] }),
}));

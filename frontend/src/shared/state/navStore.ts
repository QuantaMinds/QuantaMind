import { create } from "zustand";

/// Top-level view of the app. Each value corresponds to one of the top tabs in
/// `App.tsx`. The store is the single source of truth so the Workspace's
/// "Add Model" button can navigate to the Models tab without prop-drilling.

export type TopView =
  | "workspace"
  | "compare"
  | "inspector"
  | "models"
  | "downloads"
  | "eval"
  | "audit"
  | "agentReport"
  | "settings"
  | "docs";

interface NavStore {
  topView: TopView;
  history: TopView[];
  setTopView: (v: TopView) => void;
  goBack: () => void;
}

export const useNavStore = create<NavStore>((set) => ({
  topView: "workspace",
  history: [],
  setTopView: (v) =>
    set((s) =>
      v === s.topView ? s : { topView: v, history: [...s.history, s.topView].slice(-20) },
    ),
  goBack: () =>
    set((s) =>
      s.history.length === 0
        ? s
        : { topView: s.history[s.history.length - 1], history: s.history.slice(0, -1) },
    ),
}));

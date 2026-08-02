import { create } from "zustand";
import type { BackendKind } from "../ipc/models/storage";

/// The global model selection the app is "working on" (architecture.md rule 7).
/// Each entry carries its backend + path so consumers never re-resolve them from
/// the installed list. Single-select: every supported backend serves one model at a time (see the
/// Workspace); llama.cpp are single (the header enforces that). The whole app
/// reads this — there is no per-page model selection. A backend switch trims the
/// list to the new backend (see backendStore.setSelectedBackend).
export interface SelectedModel {
  name: string;
  backend: BackendKind;
  size_bytes: number;
  path?: string;
}

export interface SelectedModelStore {
  selectedModels: SelectedModel[];
  setSelectedModels: (m: SelectedModel[]) => void;
}

export const useSelectedModelStore = create<SelectedModelStore>((set) => ({
  selectedModels: [],
  setSelectedModels: (selectedModels) => set({ selectedModels }),
}));

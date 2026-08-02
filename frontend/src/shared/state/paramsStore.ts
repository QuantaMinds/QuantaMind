import { create } from "zustand";
import type { InferenceParams } from "../ipc/workspace/prompts";

/// Global inference parameters — the single source of truth for every run
/// (architecture.md rule 7). A key left unset is omitted entirely so the backend
/// applies its own default (and, for temperature, the per-model saved fallback).
/// Ranges/validation live at the Rust boundary (commands/prompt_options.rs); this
/// store only holds the values the header collects.
export interface ParamsStore {
  globalParams: InferenceParams;
  /// Keep the model resident across runs. Off (default) unloads it after each run
  /// — keep_alive=0; on → keep_alive=-1. Only a backend that unloads on demand honours it;
  /// llama.cpp hold their model while the sidecar runs.
  keepLoaded: boolean;
  /// When false, each model uses its own
  /// params from perModelParams instead of globalParams.
  sharedParams: boolean;
  perModelParams: Record<string, InferenceParams>;
  setKeepLoaded: (v: boolean) => void;
  setSharedParams: (v: boolean) => void;
  setModelParam: (model: string, key: keyof InferenceParams, v: number | undefined) => void;
  setParam: (key: keyof InferenceParams, v: number | undefined) => void;
  resetParam: (key: keyof InferenceParams) => void;
  reset: () => void;
}

export const useParamsStore = create<ParamsStore>((set) => ({
  globalParams: {},
  keepLoaded: false,
  sharedParams: true,
  perModelParams: {},
  setKeepLoaded: (keepLoaded) => set({ keepLoaded }),
  setSharedParams: (sharedParams) => set({ sharedParams }),
  setModelParam: (model, key, v) =>
    set((s) => {
      const m = { ...(s.perModelParams[model] ?? {}) };
      if (v === undefined) delete m[key];
      else m[key] = v;
      return { perModelParams: { ...s.perModelParams, [model]: m } };
    }),
  setParam: (key, v) =>
    set((s) => {
      const next = { ...s.globalParams };
      if (v === undefined) delete next[key];
      else next[key] = v;
      return { globalParams: next };
    }),
  resetParam: (key) =>
    set((s) => {
      const next = { ...s.globalParams };
      delete next[key];
      return { globalParams: next };
    }),
  reset: () => set({ globalParams: {}, perModelParams: {}, sharedParams: true }),
}));

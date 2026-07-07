import { create } from "zustand";
import {
  getBuiltinCollection,
  listBuiltinCollections,
  listCustomCollections,
  loadCustomCollection,
  saveCustomCollection,
  deleteCustomCollection,
  importCustomCollection,
  validateCollectionFile,
  validateCustomCollection,
  type ToolTask,
  type BuiltinCollectionInfo,
  type CollectionValidation,
} from "../../../shared/ipc/eval/registry";

/// The default read-only collection id (the first Easy-tier scenario).
export const DEFAULT_PRESET = "easy-coding";

/// Sentinel `selected` value for an unsaved, brand-new collection. It is never a
/// real preset id nor a sanitized file stem, and is never sent to the backend —
/// it only ever lives in `selected` until the user saves under a real name.
export const NEW_COLLECTION = "__new__";

/// Built-in presets can't be deleted from disk, so "removing" one just hides it
/// from the list (persisted locally). Cleared if the user wants them all back.
const HIDDEN_KEY = "qm-eval-hidden-presets";
function loadHidden(): string[] {
  try {
    return JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}
function saveHidden(ids: string[]) {
  try {
    localStorage.setItem(HIDDEN_KEY, JSON.stringify(ids));
  } catch {
    /* no localStorage (non-browser) — hide is then session-only */
  }
}

interface EvalRegistryStore {
  presets: BuiltinCollectionInfo[]; // read-only built-in collections (id + label), minus hidden
  collections: string[]; // user custom collection names
  selected: string; // a preset id OR a custom collection name
  tasks: ToolTask[]; // the active task set for the current selection
  hiddenPresets: string[]; // preset ids the user removed from the list
  /// True once the user has edited a task's env snapshot in-memory this selection. Informational
  /// only (drives the "local-only, won't publish" banner) — the fork is backend-enforced by the
  /// content-verified collection_hash. Reset whenever a selection reloads.
  edited: boolean;
  init: () => Promise<void>;
  startNew: () => void; // enter an editable, unsaved new-collection selection
  select: (idOrName: string) => Promise<void>;
  /// Save, then auto-validate: returns the fresh oracle+semantic verdict for the
  /// saved collection (best-effort — null if the validate call itself fails).
  save: (name: string, tasks: ToolTask[]) => Promise<CollectionValidation | null>;
  remove: (name: string) => Promise<void>;
  hidePreset: (id: string) => void; // "delete" a built-in preset (hide from list)
  /// Two-phase import: dry-run the full validation on the FILE first; only a clean
  /// file is imported (returns null). A failing verdict is returned WITHOUT writing
  /// anything, so the caller can show what to fix — the backend save boundary would
  /// reject it anyway, but this way the user gets the structured per-task verdict
  /// instead of a raw error string.
  importFile: (path: string) => Promise<CollectionValidation | null>;
  isPreset: (idOrName: string) => boolean;
  /// Replace a task's env snapshot (`agentic.world_state`) in memory and mark the selection edited.
  /// The edit rides to `run_batch_eval` verbatim; editing a bundled collection makes its run
  /// content-differ from pristine → `collection_hash` = None → unpublishable (fork-on-edit).
  editWorldState: (taskId: string, worldState: unknown) => void;
}

/// Holds the available datasets (read-only built-in presets + user collections)
/// and the active selection. The runner is always handed `tasks`.
export const useEvalRegistryStore = create<EvalRegistryStore>((set, get) => ({
  presets: [],
  collections: [],
  selected: DEFAULT_PRESET,
  tasks: [],
  hiddenPresets: [],
  edited: false,
  isPreset: (v) => get().presets.some((p) => p.id === v),
  init: async () => {
    const hidden = loadHidden();
    const [allPresets, collections] = await Promise.all([listBuiltinCollections(), listCustomCollections()]);
    const presets = allPresets.filter((p) => !hidden.includes(p.id));
    // Publish the picker FIRST, so a single bad default-collection load can never
    // blank the whole Built-in list (that silent failure left the page stuck on
    // "Custom JSON" with no collections). A throw below still propagates to the
    // caller, which surfaces it instead of swallowing it.
    set({ presets, collections, hiddenPresets: hidden, selected: DEFAULT_PRESET, edited: false });
    set({ tasks: await getBuiltinCollection(DEFAULT_PRESET) });
  },
  hidePreset: (id) => {
    const hidden = [...get().hiddenPresets, id];
    saveHidden(hidden);
    set({ hiddenPresets: hidden, presets: get().presets.filter((p) => p.id !== id) });
    if (get().selected === id) void get().select(DEFAULT_PRESET);
  },
  startNew: () => set({ selected: NEW_COLLECTION, tasks: [], edited: false }),
  select: async (v) => {
    const tasks = get().isPreset(v) ? await getBuiltinCollection(v) : await loadCustomCollection(v);
    set({ selected: v, tasks, edited: false });
  },
  editWorldState: (taskId, worldState) =>
    set((s) => ({
      edited: true,
      tasks: s.tasks.map((t) =>
        t.id === taskId && t.agentic ? { ...t, agentic: { ...t.agentic, world_state: worldState } } : t,
      ),
    })),
  save: async (name, tasks) => {
    await saveCustomCollection(name, tasks);
    set({ collections: await listCustomCollections() });
    await get().select(name);
    // Auto-validate what was just written so the author sees the verdict without
    // pressing Validate. Best-effort: a validate hiccup must not fail the save.
    return await validateCustomCollection(name).catch(() => null);
  },
  remove: async (name) => {
    await deleteCustomCollection(name);
    set({ collections: await listCustomCollections() });
    if (get().selected === name) await get().select(DEFAULT_PRESET);
  },
  importFile: async (path) => {
    const verdict = await validateCollectionFile(path);
    if (!verdict.ok) return verdict; // nothing written — caller shows what to fix
    const name = await importCustomCollection(path);
    set({ collections: await listCustomCollections() });
    await get().select(name);
    return null;
  },
}));

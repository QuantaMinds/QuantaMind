import { create } from "zustand";
import {
  assessReadiness,
  listReadinessProfiles,
  saveReadinessProfile,
  type AgentPath,
  type ModelVerdict,
  type ReadinessProfile,
  type RightSizingGroup,
} from "../../../shared/ipc/eval/readiness";
import {
  getHardwareSnapshot,
  getHardwareTier,
  type HardwareSnapshot,
  type HardwareTier,
} from "../../../shared/ipc/compare/hardware";
import { defaultCapBytes } from "../capBytes";

interface ReadinessStore {
  profiles: ReadinessProfile[];
  selectedProfileId: string;
  verdicts: ModelVerdict[];
  /// Right-sizing summary from the last assess (smallest still-usable quant per
  /// family). `rightSizingHint` explains an empty summary. Percent-only.
  rightSizing: RightSizingGroup[];
  rightSizingHint: string | null;
  /// Stored report columns this build couldn't interpret — surfaced beside the
  /// verdict table so a short list never reads as the complete run.
  unreadableColumns: number;
  hardware: HardwareSnapshot | null;
  /// Hardware class + recommended difficulty tier (Phase 9B) — the Agent Report's
  /// Executive Verdict shows this as the advisory hardware lens. Best-effort; `null`
  /// just omits the lens.
  hardwareTier: HardwareTier | null;
  /// The model the deep-dive (Executive Verdict / Tier Matrix / Failure Taxonomy) targets.
  /// Defaults to the recommended verdict; a table-row click reassigns it.
  focusedModel: string;
  focusedPath: AgentPath;
  capBytes: number | null;
  /// True once an assess has completed — distinguishes "not run yet" from a
  /// genuinely empty result (no persisted report) so the page shows the right state.
  assessed: boolean;
  loading: boolean;
  error: string | null;
  loadProfiles: () => Promise<void>;
  loadHardware: () => Promise<void>;
  loadHardwareTier: () => Promise<void>;
  setFocus: (model: string, path: AgentPath) => void;
  selectProfile: (id: string) => void;
  setCap: (bytes: number) => void;
  assess: (collectionId: string) => Promise<void>;
  /// Persist edited thresholds to disk (Rust = source of truth) then reload the
  /// profile list so the active profile reflects the new gates.
  saveProfile: (profile: ReadinessProfile) => Promise<void>;
}

/// Transient readiness state — profiles + the current verdicts. Source of truth
/// is Rust (profiles on disk, verdicts computed by `assess_readiness`); this store
/// holds none of it persistently.
export const useReadinessStore = create<ReadinessStore>((set, get) => ({
  profiles: [],
  selectedProfileId: "",
  verdicts: [],
  rightSizing: [],
  rightSizingHint: null,
  unreadableColumns: 0,
  hardware: null,
  hardwareTier: null,
  focusedModel: "",
  focusedPath: "prompt_based",
  capBytes: null,
  assessed: false,
  loading: false,
  error: null,
  loadProfiles: async () => {
    try {
      const profiles = await listReadinessProfiles();
      set((s) => ({ profiles, selectedProfileId: s.selectedProfileId || profiles[0]?.id || "" }));
    } catch (e) {
      set({ error: String(e) });
    }
  },
  loadHardware: async () => {
    // Best-effort: a missing snapshot just leaves VRAM fit unmeasured.
    try {
      const hardware = await getHardwareSnapshot();
      set((s) => ({ hardware, capBytes: s.capBytes ?? defaultCapBytes(hardware) }));
    } catch {
      /* no hardware snapshot — fit stays unmeasured */
    }
  },
  loadHardwareTier: async () => {
    // Best-effort: a missing tier just omits the Executive Verdict's hardware lens.
    try {
      set({ hardwareTier: await getHardwareTier() });
    } catch {
      /* no hardware tier — the advisory context is simply hidden */
    }
  },
  setFocus: (model, path) => set({ focusedModel: model, focusedPath: path }),
  selectProfile: (id) => set({ selectedProfileId: id, assessed: false, verdicts: [], rightSizing: [], rightSizingHint: null, unreadableColumns: 0 }),
  setCap: (bytes) => set({ capBytes: bytes }),
  assess: async (collectionId) => {
    const { selectedProfileId, capBytes } = get();
    if (!selectedProfileId) return;
    set({ loading: true, error: null });
    try {
      const a = await assessReadiness(collectionId, selectedProfileId, capBytes ?? undefined);
      set({ verdicts: a.verdicts, rightSizing: a.right_sizing, rightSizingHint: a.right_sizing_hint ?? null, unreadableColumns: a.unreadable_columns, assessed: true, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false, assessed: false });
    }
  },
  saveProfile: async (profile) => {
    await saveReadinessProfile(profile);
    await get().loadProfiles();
  },
}));

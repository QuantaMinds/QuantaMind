import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  runContextCliff,
  stopContextCliff,
  getCliffResults,
  EVENT_CLIFF_PROGRESS,
  EVENT_CLIFF_STEP,
  type CliffSource,
  type CliffProgress,
  type CliffStep,
} from "../../../shared/ipc/eval/cliff";
import { formatIpcError } from "../../../shared/ipc/core/error";
import { type CliffPoint } from "../cliff";
import type { ToolTask } from "../../../shared/ipc/eval/registry";
import type { BackendKind } from "../../../shared/ipc/models/storage";
import type { AgentPath } from "../../../shared/ipc/eval/readiness";
import type { InferenceParams } from "../../../shared/ipc/workspace/prompts";

/// What the Matrix carries to the Audit panel so the probe lands pre-filled
/// (NEVER auto-run — guardrail 1).
export interface CliffRequest {
  model: string;
  backend: BackendKind;
  collectionId: string;
  maxTokens: number;
  /// Pre-filled ladder depth (Test Steps) so the panel lands fully ready — the user
  /// can still adjust it before clicking Execute.
  steps: number;
  /// GGUF path for a llama.cpp model, carried so the probe can match the RUNNING server by
  /// its exact launch path. Without it the override drops the path → the backend compares the
  /// server's real path against "" → false "WrongModel" ("Start llama.cpp with …"). Absent for Ollama.
  path?: string;
}

export interface RunProbeArgs {
  model: string;
  backend: BackendKind;
  collectionId: string;
  tasks: ToolTask[];
  maxTokens: number;
  steps: number;
  /// Which padding fills the context: an embedded synthetic preset or the user's
  /// own text. The backend engine cycles it, char-boundary-safe, to each verified depth.
  source: CliffSource;
  params?: InferenceParams;
  /// The GGUF path of the selected model (llama.cpp only). The backend matches it
  /// against the running llama-server so the probe can refuse to run when the wrong
  /// model is loaded or its launch `-c` is too small — rather than 400 on every rung.
  modelPath?: string;
  /// Which tool-calling path to probe on, chosen by the user on the test page:
  /// `"native_fc"` drives native function-calling, `"prompt_based"` the JSON-in-text proxy.
  /// A single-method probe (not the batch's run-both), so it's a scalar, not two booleans.
  method: AgentPath;
}

interface CliffStore {
  /// A pending pre-fill request set by the Matrix, consumed by the panel.
  request: CliffRequest | null;
  /// The live probe series (one point per rung) — held in the store so the run
  /// survives tab navigation.
  points: CliffPoint[];
  running: boolean;
  /// The model currently being probed (for the Matrix "probing…" indicator).
  runningModel: string | null;
  progress: { done: number; total: number };
  /// Monotonic overall completion fraction in [0,1], derived from `progress` + `step` by
  /// [`progressFraction`]. Held in the store (not recomputed in the panel) so it can stay
  /// non-decreasing across a run — a re-sweep never drives the bar backward. Reset to 0 at
  /// the start of each run; snapped to 1 on completion (an early-stopped probe is still done).
  frac: number;
  /// The latest fine-grained sub-rung step (per task generation). Null between runs and
  /// until the first task of a run completes — the panel uses it to show "rung r/N ·
  /// position p/3 · task t/M" and an ETA so a slow deep rung never looks frozen.
  step: CliffStep | null;
  /// Wall-clock ms when the current run started (Date.now()). Drives the ETA; null when idle.
  startedAt: number | null;
  error: string | null;
  /// Backend-hydrated cliff depths: collection → model (verbatim key) → tokens.
  results: Record<string, Record<string, number>>;
  /// (collection → model) probes that COMPLETED this session, including ones that found
  /// no cliff (so the Matrix can distinguish "probed, healthy" from "not probed"). Held
  /// in-session only — cross-session persistence of the no-cliff case is a follow-up.
  probed: Record<string, Record<string, boolean>>;
  /// (collection → model) probes whose UNPADDED baseline (rung 0) was already below the
  /// pass bar — the model fails from the start, so "✓ no cliff" would be a lie. The Matrix
  /// renders this as a failure state, not a healthy one. In-session only, like `probed`.
  brokenBaseline: Record<string, Record<string, boolean>>;

  setRequest: (req: CliffRequest) => void;
  consumeRequest: () => CliffRequest | null;
  hydrate: (collectionId: string) => Promise<void>;
  cliffFor: (collectionId: string, model: string) => number | null;
  /// The deepest measured cliff for a model across ALL collections — for the
  /// Inspector gauge, which has a model but no collection in scope.
  cliffForModel: (model: string) => number | null;
  /// Did a probe complete this session for (collection, model)? True even when no cliff
  /// was found — so the Matrix shows "✓ no cliff" rather than "Run probe ↗".
  wasProbed: (collectionId: string, model: string) => boolean;
  /// Did the probe's baseline fail (broken from the start)? When true the Matrix must NOT
  /// claim "✓ no cliff" — the model never had a healthy plateau to fall off.
  hasBrokenBaseline: (collectionId: string, model: string) => boolean;
  runProbe: (args: RunProbeArgs) => Promise<void>;
  stop: () => void;
  reset: () => void;
}

// Module-level generation token: bumping it invalidates any in-flight run (stop /
// supersede), so a long sweep can be cancelled and a re-run never races the old one.
let activeRun = 0;

/// Within-rung fill ceiling: one sweep advances the bar to at most this fraction of the
/// current rung's slice, reserving headroom for a possible verify-and-adjust re-sweep —
/// so the bar never claims a rung is finished before the authoritative `cliff-progress`
/// (`on_rung`) event confirms it.
const RUNG_FILL_CAP = 0.9;

/// Overall completion fraction in [0,1]. Rung boundaries are anchored on the authoritative
/// per-rung counter (`done`/`total` from `cliff-progress`); the fine-grained `step` only
/// fills WITHIN the current incomplete rung, capped below the boundary. The caller clamps
/// the result to never decrease across a run — a verify-and-adjust re-sweep resets the
/// `step` counters, but the bar must not jump backward.
function progressFraction(done: number, total: number, step: CliffStep | null): number {
  if (total <= 0) return 0;
  let within = 0;
  // Only the CURRENT incomplete rung gets within-rung fill: once `on_rung` advances `done`
  // to this rung, the step is stale and must add nothing (else it double-counts the rung
  // `done` already includes, overshooting past the boundary).
  if (step != null && step.rung === done + 1 && step.total_positions > 0 && step.total_tasks > 0) {
    const swept = ((step.position - 1) * step.total_tasks + step.task) / (step.total_positions * step.total_tasks);
    within = Math.min(swept, RUNG_FILL_CAP);
  }
  return Math.min(1, (done + within) / total);
}

export const useCliffStore = create<CliffStore>((set, get) => ({
  request: null,
  points: [],
  running: false,
  runningModel: null,
  progress: { done: 0, total: 0 },
  frac: 0,
  step: null,
  startedAt: null,
  error: null,
  results: {},
  probed: {},
  brokenBaseline: {},

  setRequest: (req) => set({ request: req }),
  consumeRequest: () => {
    const req = get().request;
    if (req) set({ request: null });
    return req;
  },

  hydrate: async (collectionId) => {
    try {
      const map = await getCliffResults(collectionId); // model → CliffStatus
      // Restore ALL states so broken/no-cliff survive a reload, not just collapse depths:
      // results = collapse depths; probed = any probed state; brokenBaseline = Broken.
      const results: Record<string, number> = {};
      const probed: Record<string, boolean> = {};
      const broken: Record<string, boolean> = {};
      for (const [m, st] of Object.entries(map)) {
        if (st.status === "NotProbed") continue;
        probed[m] = true;
        if (st.status === "Collapsed") results[m] = st.depth;
        else if (st.status === "Broken") broken[m] = true;
      }
      set((s) => ({
        results: { ...s.results, [collectionId]: results },
        probed: { ...s.probed, [collectionId]: probed },
        brokenBaseline: { ...s.brokenBaseline, [collectionId]: broken },
      }));
    } catch (e) {
      // best-effort — a missing/unreadable store just leaves the cell N/A — but log it.
      console.error("cliff hydrate failed:", e);
    }
  },

  cliffFor: (collectionId, model) => get().results[collectionId]?.[model] ?? null,
  cliffForModel: (model) => {
    const found = Object.values(get().results)
      .map((byModel) => byModel[model])
      .filter((v): v is number => v != null);
    return found.length ? Math.max(...found) : null;
  },
  wasProbed: (collectionId, model) => get().probed[collectionId]?.[model] === true,
  hasBrokenBaseline: (collectionId, model) => get().brokenBaseline[collectionId]?.[model] === true,

  runProbe: async ({ model, backend, collectionId, tasks, maxTokens, steps, source, params, modelPath, method }) => {
    // GUARDRAIL 2: clear all prior state BEFORE dispatching — never append to a
    // stale series (that corrupts the chart and the persisted cliff).
    const myRun = ++activeRun;
    set({ points: [], error: null, running: true, runningModel: model, progress: { done: 0, total: steps }, frac: 0, step: null, startedAt: Date.now() });

    // Live per-rung points stream from the backend engine over `cliff-progress`; the
    // engine owns the ladder, padding, verify-and-adjust, classification, and
    // persistence (the Matrix/verdict read the stored status afterward). A second,
    // finer `cliff-step` stream ticks once per task generation so the UI moves DURING a
    // slow padded rung instead of freezing between the (minutes-apart) rung events.
    const unlisteners: UnlistenFn[] = [];
    try {
      unlisteners.push(
        await listen<CliffProgress>(EVENT_CLIFF_PROGRESS, (ev) => {
          const p = ev.payload;
          // Filter by run token, not model: two runs of the SAME model must not bleed into
          // each other. A superseded run's late (often cancelled/partial) events carry the
          // OLD run_id and are dropped here, so they never pollute the new run's chart.
          if (activeRun !== myRun || p.run_id !== myRun) return;
          set((s) => ({
            // verified_tokens 0 ⇒ the backend reported no count for this rung — render
            // it as "not reported", never a fake ≈0-token depth.
            points: [...s.points, { promptTokens: p.point.verified_tokens || null, composite: p.point.composite, passed: p.point.passed, trials: p.point.trials, trace: p.point.trace }],
            progress: { done: p.done, total: p.total },
            // Advance the bar to the just-completed rung's boundary; never backward.
            frac: Math.max(s.frac, progressFraction(p.done, p.total, s.step)),
          }));
        }),
      );
      unlisteners.push(
        await listen<CliffStep>(EVENT_CLIFF_STEP, (ev) => {
          const st = ev.payload;
          // Same run-token filter as the rung stream — a superseded run's late ticks must
          // never drive the live line/ETA of the new run.
          if (activeRun !== myRun || st.run_id !== myRun) return;
          // Fill within the current rung, monotonically — a re-sweep resets the step
          // counters but the bar holds (never claims a rung done before `on_rung`).
          set((s) => ({ step: st, frac: Math.max(s.frac, progressFraction(s.progress.done, s.progress.total, st)) }));
        }),
      );

      const report = await runContextCliff(model, backend, collectionId, tasks, source, maxTokens, steps, params, myRun, modelPath, method === "native_fc");
      if (activeRun !== myRun) return; // stopped or superseded mid-run

      // The report is authoritative — replace the live series with its verified rungs
      // so the chart and the (backend-persisted) status can never disagree.
      const points: CliffPoint[] = report.points.map((p) => ({ promptTokens: p.verified_tokens || null, composite: p.composite, passed: p.passed, trials: p.trials, trace: p.trace }));
      const broken = report.status.status === "Broken";
      set((s) => {
        const col = { ...(s.results[collectionId] ?? {}) };
        // `results` holds GENUINE collapse depths only — set it for a real cliff, clear
        // it otherwise (no-cliff / broken render via their own flags, never a number).
        if (report.status.status === "Collapsed") col[model] = report.status.depth;
        else delete col[model];
        return {
          points,
          // The probe returned — it's genuinely done even if it early-stopped before the
          // last rung, so snap the bar to 100% rather than leaving it short.
          frac: 1,
          // Mark probed REGARDLESS of outcome so the Matrix shows "✓ no cliff" rather than
          // an unmeasured "Run probe ↗"; record broken explicitly so a healthy re-run clears it.
          probed: { ...s.probed, [collectionId]: { ...(s.probed[collectionId] ?? {}), [model]: true } },
          brokenBaseline: { ...s.brokenBaseline, [collectionId]: { ...(s.brokenBaseline[collectionId] ?? {}), [model]: broken } },
          results: { ...s.results, [collectionId]: col },
        };
      });
    } catch (e) {
      if (activeRun === myRun) set((s) => ({ error: s.error ?? formatIpcError(e) }));
    } finally {
      for (const u of unlisteners) u();
      if (activeRun === myRun) set({ running: false, runningModel: null, step: null, startedAt: null });
    }
  },

  stop: () => {
    activeRun++; // invalidate the in-flight run (the awaited result is ignored)
    // Actually cancel the backend probe — without this the model kept being called
    // through the whole ladder; bumping activeRun only hid the result in the UI.
    void stopContextCliff().catch((e) => console.error("stop context-cliff failed:", e));
    set({ running: false, runningModel: null, frac: 0, step: null, startedAt: null });
  },

  reset: () => {
    activeRun++;
    // Reset abandons any in-flight run too — cancel the backend so it doesn't keep
    // calling the model. Only when actually running, so idle selection-change resets
    // don't fire a spurious IPC.
    if (get().running) {
      void stopContextCliff().catch((e) => console.error("stop context-cliff failed:", e));
    }
    set({ points: [], error: null, running: false, runningModel: null, progress: { done: 0, total: 0 }, frac: 0, step: null, startedAt: null });
  },
}));

import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("../../../shared/ipc/eval/cliff", () => ({
  runContextCliff: vi.fn(),
  stopContextCliff: vi.fn().mockResolvedValue(undefined),
  getCliffResults: vi.fn(),
  EVENT_CLIFF_PROGRESS: "cliff-progress",
  EVENT_CLIFF_STEP: "cliff-step",
}));

import { listen } from "@tauri-apps/api/event";
import { runContextCliff, stopContextCliff, getCliffResults } from "../../../shared/ipc/eval/cliff";
import type { CliffStep } from "../../../shared/ipc/eval/cliff";
import { useCliffStore } from "../state/cliffStore";
import type { RunProbeArgs } from "../state/cliffStore";
import type { ToolTask } from "../../../shared/ipc/eval/registry";

const task: ToolTask = { id: "t", category: "single", prompt: "p", tools: [], expected: { type: "call", name: "x", args: {} } };
const args = (over: Partial<RunProbeArgs> = {}): RunProbeArgs => ({
  model: "qwen2.5-coder:7b",
  backend: "ollama",
  collectionId: "finance",
  tasks: [task],
  maxTokens: 8000,
  steps: 3,
  source: { kind: "preset", preset: "corporate_policy" },
  method: "prompt_based",
  ...over,
});

// Backend CliffPoint / CliffReport shapes.
const rung = (verified_tokens: number, composite: number | null) => ({
  target_tokens: verified_tokens, verified_tokens, composite, per_depth: [],
});
type Status = { status: "Collapsed"; depth: number } | { status: "NoCliff"; tested: number } | { status: "Broken"; tested: number };
const reportOf = (status: Status, cliff_tokens: number | null, points: ReturnType<typeof rung>[]) => ({ points, status, cliff_tokens });

const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  vi.clearAllMocks();
  useCliffStore.getState().reset();
  useCliffStore.setState({ results: {}, request: null, probed: {}, brokenBaseline: {} });
});

describe("cliffStore", () => {
  it("runs the backend probe and updates results from the classified status (verbatim model key)", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Collapsed", depth: 8000 }, 4000, [rung(1000, 1.0), rung(4000, 1.0), rung(8000, 0.5)]) as never,
    );

    await useCliffStore.getState().runProbe(args());

    const s = useCliffStore.getState();
    expect(s.points).toHaveLength(3);
    expect(s.running).toBe(false);
    // The backend builds the ladder + pads + classifies; the store just forwards.
    const call = vi.mocked(runContextCliff).mock.calls[0];
    expect(call[0]).toBe("qwen2.5-coder:7b"); // model
    expect(call[1]).toBe("ollama"); // backend
    expect(call[2]).toBe("finance"); // collectionId
    expect(call[4]).toEqual({ kind: "preset", preset: "corporate_policy" }); // source
    expect(call[5]).toBe(8000); // maxTokens
    expect(call[6]).toBe(3); // steps
    expect(call[10]).toBe(false); // runNativeFc — "prompt_based" method
    expect(s.cliffFor("finance", "qwen2.5-coder:7b")).toBe(8000); // collapse depth, colon key preserved
  });

  it("forwards the chosen method to the backend as runNativeFc", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 4000 }, 4000, [rung(4000, 1.0)]) as never);
    await useCliffStore.getState().runProbe(args({ method: "native_fc" }));
    expect(vi.mocked(runContextCliff).mock.calls[0][10]).toBe(true); // native_fc → runNativeFc true
  });

  it("resets the series before a re-run — never appends to the old run (guardrail 2)", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 1000 }, 1000, [rung(1000, 1.0), rung(1000, 1.0)]) as never,
    );
    await useCliffStore.getState().runProbe(args({ steps: 2 }));
    expect(useCliffStore.getState().points).toHaveLength(2);
    await useCliffStore.getState().runProbe(args({ steps: 2 }));
    expect(useCliffStore.getState().points).toHaveLength(2); // 2, not 4 — cleared first
  });

  it("marks a NO-cliff result probed but carries no collapse depth", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "NoCliff", tested: 1000 }, 1000, [rung(1000, 1.0), rung(1000, 1.0), rung(1000, 1.0)]) as never,
    );
    await useCliffStore.getState().runProbe(args({ steps: 3 }));
    const s = useCliffStore.getState();
    expect(s.wasProbed("finance", "qwen2.5-coder:7b")).toBe(true);
    expect(s.hasBrokenBaseline("finance", "qwen2.5-coder:7b")).toBe(false);
    expect(s.cliffFor("finance", "qwen2.5-coder:7b")).toBeNull(); // no collapse depth
  });

  it("flags a broken baseline and keeps it OUT of the results depth map", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(
      reportOf({ status: "Broken", tested: 388 }, null, [rung(388, 0.0), rung(388, 0.0)]) as never,
    );
    await useCliffStore.getState().runProbe(args({ steps: 2 }));
    const s = useCliffStore.getState();
    expect(s.wasProbed("finance", "qwen2.5-coder:7b")).toBe(true);
    expect(s.hasBrokenBaseline("finance", "qwen2.5-coder:7b")).toBe(true);
    expect(s.cliffFor("finance", "qwen2.5-coder:7b")).toBeNull(); // never a misleading depth
  });

  it("a healthy re-run clears a stale broken-baseline flag", async () => {
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "Broken", tested: 388 }, null, [rung(388, 0.0)]) as never);
    await useCliffStore.getState().runProbe(args({ steps: 2 }));
    expect(useCliffStore.getState().hasBrokenBaseline("finance", "qwen2.5-coder:7b")).toBe(true);
    vi.mocked(runContextCliff).mockResolvedValue(reportOf({ status: "NoCliff", tested: 4000 }, 4000, [rung(4000, 1.0)]) as never);
    await useCliffStore.getState().runProbe(args({ steps: 2 }));
    expect(useCliffStore.getState().hasBrokenBaseline("finance", "qwen2.5-coder:7b")).toBe(false);
  });

  it("stop halts an in-flight run before it writes any state", async () => {
    let resolveRun: (v: unknown) => void = () => {};
    vi.mocked(runContextCliff).mockImplementation(() => new Promise((r) => (resolveRun = r)) as never);
    const p = useCliffStore.getState().runProbe(args({ steps: 3 }));
    await flush(); // probe in flight
    useCliffStore.getState().stop();
    expect(useCliffStore.getState().running).toBe(false);
    // Stop must actually cancel the BACKEND probe — not just hide the result in the UI.
    expect(stopContextCliff).toHaveBeenCalledTimes(1);
    resolveRun(reportOf({ status: "Collapsed", depth: 8000 }, 4000, [rung(8000, 0.5)])); // resolves AFTER stop
    await p;
    // Superseded run must not write results/probed.
    expect(useCliffStore.getState().cliffFor("finance", "qwen2.5-coder:7b")).toBeNull();
    expect(useCliffStore.getState().wasProbed("finance", "qwen2.5-coder:7b")).toBe(false);
  });

  it("drops cliff-progress events from a superseded run (run_id mismatch)", async () => {
    // Capture the progress listener BY EVENT NAME — the store registers two listeners
    // (cliff-progress + cliff-step), so grabbing "the last fn" would catch the wrong one.
    type ProgressCb = (ev: { payload: unknown }) => void;
    const h: { cb: ProgressCb | null } = { cb: null };
    vi.mocked(listen).mockImplementation((event, fn) => {
      if (event === "cliff-progress") h.cb = fn as unknown as ProgressCb;
      return Promise.resolve(() => {});
    });
    let resolveRun: (v: unknown) => void = () => {};
    vi.mocked(runContextCliff).mockImplementation(() => new Promise((r) => (resolveRun = r)) as never);

    const p = useCliffStore.getState().runProbe(args({ steps: 3 }));
    await flush(); // listener registered
    // The run token handed to the backend is runContextCliff's last argument.
    const myRunId = vi.mocked(runContextCliff).mock.calls[0][8] as number;

    // An event tagged with THIS run's id is folded into the series…
    h.cb?.({ payload: { run_id: myRunId, model: "qwen2.5-coder:7b", done: 1, total: 3, point: rung(1000, 1.0) } });
    expect(useCliffStore.getState().points).toHaveLength(1);

    // …but a (same-model) event from a DIFFERENT run is ignored — no cross-run pollution.
    h.cb?.({ payload: { run_id: myRunId + 999, model: "qwen2.5-coder:7b", done: 2, total: 3, point: rung(2000, 0.1) } });
    expect(useCliffStore.getState().points).toHaveLength(1);

    resolveRun(reportOf({ status: "NoCliff", tested: 1000 }, 1000, [rung(1000, 1.0)]));
    await p;
  });

  it("surfaces a backend error instead of a silent blank chart", async () => {
    vi.mocked(runContextCliff).mockRejectedValue(new Error("server down"));
    await useCliffStore.getState().runProbe(args());
    expect(useCliffStore.getState().error).toMatch(/server down/);
    expect(useCliffStore.getState().running).toBe(false);
  });

  it("setRequest / consumeRequest is one-shot (pre-fill carried once)", () => {
    useCliffStore.getState().setRequest({ model: "m", backend: "ollama", collectionId: "c", maxTokens: 8000, steps: 5 });
    expect(useCliffStore.getState().consumeRequest()?.model).toBe("m");
    expect(useCliffStore.getState().request).toBeNull();
    expect(useCliffStore.getState().consumeRequest()).toBeNull();
  });

  it("hydrate restores every cliff state (depth/no-cliff/broken) across a reload", async () => {
    vi.mocked(getCliffResults).mockResolvedValue({
      "qwen2.5-coder:7b": { status: "Collapsed", depth: 12000 }, // verbatim colon key
      "held:7b": { status: "NoCliff", tested: 4000 },
      "broke:7b": { status: "Broken", tested: 388 },
    });
    await useCliffStore.getState().hydrate("finance");
    const s = useCliffStore.getState();
    expect(s.cliffFor("finance", "qwen2.5-coder:7b")).toBe(12000); // collapse depth in results
    expect(s.cliffFor("finance", "held:7b")).toBeNull(); // no-cliff has no depth
    expect(s.wasProbed("finance", "held:7b")).toBe(true); // ...but it survives as "probed"
    expect(s.wasProbed("finance", "broke:7b")).toBe(true);
    expect(s.hasBrokenBaseline("finance", "broke:7b")).toBe(true); // broken survives the reload
  });

  it("cliffForModel returns the DEEPEST cliff across all collections (Inspector has no collection)", () => {
    useCliffStore.setState({ results: { finance: { "m:1b": 6000 }, ops: { "m:1b": 9000 }, misc: { other: 1 } } });
    expect(useCliffStore.getState().cliffForModel("m:1b")).toBe(9000); // max(6000, 9000)
    expect(useCliffStore.getState().cliffForModel("never-probed")).toBeNull();
  });
});

// The progress fraction (`frac`) is what drives the bar/percentage. It must be monotonic
// (a verify-and-adjust re-sweep resets the step counters but must NOT walk the bar back)
// and must never claim a rung is finished — false 100% then a drop to ~80% — before the
// authoritative `cliff-progress` (`on_rung`) event advances `done`.
describe("cliffStore progress fraction (monotonic, no false 100%)", () => {
  // Capture BOTH listeners by event name so we can drive `cliff-step` and `cliff-progress`
  // independently — the bug only surfaces in their interplay across a rung.
  type Cb = (ev: { payload: unknown }) => void;
  const handlers: Record<string, Cb> = {};
  const startRun = async () => {
    for (const k of Object.keys(handlers)) delete handlers[k];
    vi.mocked(listen).mockImplementation((event, fn) => {
      handlers[event as string] = fn as unknown as Cb;
      return Promise.resolve(() => {});
    });
    let resolveRun: (v: unknown) => void = () => {};
    vi.mocked(runContextCliff).mockImplementation(() => new Promise((r) => (resolveRun = r)) as never);
    const done = useCliffStore.getState().runProbe(args({ steps: 2 }));
    await flush(); // both listeners registered
    const runId = vi.mocked(runContextCliff).mock.calls[0][8] as number;
    return { runId, done, resolve: (r: unknown) => resolveRun(r) };
  };
  const fireStep = (runId: number, s: Partial<CliffStep>) =>
    handlers["cliff-step"]?.({
      payload: { run_id: runId, model: "qwen2.5-coder:7b", rung: 1, total_rungs: 2, target_tokens: 0, position: 1, total_positions: 1, task: 1, total_tasks: 5, ...s },
    });
  const fireRung = (runId: number, doneN: number, total: number) =>
    handlers["cliff-progress"]?.({ payload: { run_id: runId, model: "qwen2.5-coder:7b", done: doneN, total, point: rung(1000, 1.0) } });
  const frac = () => useCliffStore.getState().frac;

  it("never falsely hits 100% mid-rung and never walks backward on a re-sweep", async () => {
    const { runId, done, resolve } = await startRun();
    const seq: number[] = [];
    const record = () => seq.push(frac());

    // Rung 1 (baseline, single position, 5 tasks): fills toward — but not past — the 1/2 boundary.
    for (let t = 1; t <= 5; t++) { fireStep(runId, { rung: 1, total_positions: 1, task: t }); record(); }
    expect(frac()).toBeLessThan(0.5); // capped below the rung boundary, never claims rung done
    fireRung(runId, 1, 2); record();
    expect(frac()).toBeCloseTo(0.5, 5); // on_rung snaps to the real boundary

    // Rung 2 (last, 3 positions × 5 tasks): sweep 1 climbs to the very end of the sweep…
    for (let p = 1; p <= 3; p++) for (let t = 1; t <= 5; t++) { fireStep(runId, { rung: 2, total_positions: 3, position: p, task: t }); record(); }
    const afterSweep1 = frac();
    expect(afterSweep1).toBeLessThan(1); // THE BUG GUARD: one sweep must NOT read 100%
    expect(afterSweep1).toBeGreaterThan(0.5);

    // …then a verify-and-adjust SECOND sweep resets position/task to the start.
    for (let p = 1; p <= 3; p++) for (let t = 1; t <= 5; t++) { fireStep(runId, { rung: 2, total_positions: 3, position: p, task: t }); record(); }
    expect(frac()).toBe(afterSweep1); // monotonic: the bar held, never dropped to ~80%

    fireRung(runId, 2, 2); record();
    expect(frac()).toBe(1); // only NOW, when the rung is authoritatively done, is it 100%

    // The whole emitted sequence is non-decreasing and bounded in [0,1] (data-quality check).
    for (let i = 1; i < seq.length; i++) expect(seq[i]).toBeGreaterThanOrEqual(seq[i - 1]);
    for (const f of seq) { expect(f).toBeGreaterThanOrEqual(0); expect(f).toBeLessThanOrEqual(1); }

    resolve(reportOf({ status: "NoCliff", tested: 1000 }, 1000, [rung(1000, 1.0), rung(1000, 1.0)]));
    await done;
  });

  it("snaps to 100% when an early-stopped probe completes below the last rung", async () => {
    const { runId, done, resolve } = await startRun();
    // Only rung 1 of 2 ever ran (early-stop on a broken baseline) — frac is still mid-run…
    fireStep(runId, { rung: 1, task: 3 });
    fireRung(runId, 1, 2);
    expect(frac()).toBeLessThan(1);
    // …but completing the probe is genuinely done, so the bar reads 100%.
    resolve(reportOf({ status: "Broken", tested: 388 }, null, [rung(388, 0.0)]));
    await done;
    expect(frac()).toBe(1);
  });

  it("resets frac to 0 on a new run, on reset, and on stop", async () => {
    useCliffStore.setState({ frac: 0.7 });
    useCliffStore.getState().reset();
    expect(frac()).toBe(0);

    const { runId, done, resolve } = await startRun();
    fireStep(runId, { rung: 1, task: 4 });
    expect(frac()).toBeGreaterThan(0);
    useCliffStore.getState().stop();
    expect(frac()).toBe(0); // stop clears the bar
    resolve(reportOf({ status: "NoCliff", tested: 1000 }, 1000, [rung(1000, 1.0)]));
    await done;
  });
});

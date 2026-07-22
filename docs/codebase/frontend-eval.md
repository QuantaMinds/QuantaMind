# Frontend — The Tests Tab

The largest frontend feature. Lives under `frontend/src/features/eval/`. It scores
**local LLMs** on tool-calling and agentic ability and surfaces the results that
downstream features (Agent-Readiness verdicts, Quant) consume.

Cross-links:
- Backend scoring engine → [`backend-eval-engine.md`](./backend-eval-engine.md)
- Result persistence (collections, run logs, cliff store) → [`backend-persistence.md`](./backend-persistence.md)
- Consumers of eval results (readiness gauge, quant) → [`frontend-inspector-quant-agentreport.md`](./frontend-inspector-quant-agentreport.md)
- App shell / nav / stores → [`frontend-overview.md`](./frontend-overview.md)

---

## Overview

**Why.** A model that passes a chat benchmark can still be useless as an *agent*:
it emits malformed tool JSON, calls the wrong tool, loops forever, hallucinates
"done", or degrades as the context fills. The Tests tab measures exactly those
failure modes on the user's own machine, against the user's own models, with no
fabricated numbers (every "N/A"/"—" is a real absence, never a faked 0).

**Modes.** All run over a *collection* of `ToolTask`s (a curated built-in preset
or a user-authored custom set):

| Mode | What it measures | Where |
|---|---|---|
| **Tool-call (single-turn)** | One prompt → one (or parallel) tool call. Parse / tool-match / args-match / abstain sub-scores → `composite`. | matrix grid, scoreboard, PipelinePanel trace |
| **Agentic (multi-step)** | A sandbox loop: model calls tools until an end-state, with injected faults. `Pass^k` (all k runs pass), avg steps, effort (tokens), schema-resilience, top-error. | scoreboard, TraceDebugger |
| **Matrix (across models)** | Run a whole collection across many target models → one row per model, compared side-by-side. | PerformanceMatrix / MatrixPanel |
| **Context Stress Test** | Pad the context to growing depths, find where tool-call accuracy collapses. Feeds the readiness verdict. | ContextCliffPanel + Chart (Audit tab) |
| **Custom collections + CSV** | Author tasks in-app; import single-turn cases from CSV; import/export JSON. | EvalManager, CollectionEditor, CsvImportModal |

**How.** Every panel is a thin React shell over a Tauri IPC command in
`shared/ipc/eval/*`. Long runs stream Tauri **events** (`batch-progress`,
`agentic-step`, `batch-complete`, `cliff-progress`, `cliff-step`) into a Zustand store; the
heavy report lands once on completion. Crash-recovery (`check_unfinished_run` →
`resume_batch_eval`) lets an interrupted batch resume.

### Panel → IPC → store map

| Panel / group | Shows | IPC command(s) | Store |
|---|---|---|---|
| **EvalPage** | The Eval-tab layout (Manager + Scoreboard + TraceDebugger + PerformanceMatrix) | — (orchestrates) | all four |
| **EvalManager** | Difficulty-tier–filtered collection picker (three domains per tier — the backend lists only the curated 12), model, editable k / maxSteps, native-FC toggle, **Thinking-Budget preset** (Lean/Standard/Deep — fixed presets, not a slider, so verdicts stay reproducible; shows the resolved per-tier token number, threaded to `run_batch_eval` as `thinkPreset` and stamped on the verdict), **single-task selection** (see below), Run/Stop, New Collection/Import/Export/Validate | `run_batch_eval` / `stop_batch_eval` (via `useBatchRun`) | `evalRegistryStore`, `batchStore` |
| **MatrixScoreboard** ("Simulator") | Per-task Pass/Fail/Partial table + live progress (read-only; authoring lives in the sidebar). Scopes to a single task's row + aggregate when `runTaskId` is set | reads streamed events | `batchStore`, `evalRegistryStore` |
| **TraceDebugger** ("Evaluator") | One (model,task) pipeline: Config→System→Stream→Verify + agentic step timeline | reads cached outcome/steps | `batchStore`, `evalRegistryStore` |
| **PerformanceMatrix** | One row per model: Pass^k, native FC, avg-steps, effort, **Tokens/Task** (T* — total tokens ÷ completions, incl. failed-run waste; ≥ Effort), schema-resil, **context limit**, top-error. Renders **BoundaryPanel** below the matrix when the collection carries Category-K tasks | reads `report`; pre-fills cliff | `batchStore`, `cliffStore` |
| **BoundaryPanel** *(Category K)* | Per config: resistance + over-refusal (over decisive benign runs; capability-failed runs shown as excluded; honest em-dash when an arm didn't decide), Pass/Fail/Inconclusive gate, model/config/unattributed attribution split, per-vector breakdown, and the static-set caveat (rendered prominently). Kept off the capability axis | reads `AggAgentic.boundary` off `report` | — |
| **CollectionEditor** | Task list + Task/Sandbox configurator (authoring) | registry CRUD (via store) | `evalRegistryStore` |
| **CsvImportModal** | Live-validated CSV → tasks | `read_text_capped`, `import`/`save_custom_collection` | `evalRegistryStore` |
| **ContextCliffPanel** + Chart | Cliff probe controls, rung table, accuracy-vs-depth chart. A **Native FC / Prompt-based** method toggle (default native; disabled → prompt-only on MLX) picks the tool-calling path the probe measures. A pre-flight amber banner (`cliff-fit-warning`) warns before Execute when the requested depth won't fit device memory on Ollama (`useHardwareSnapshot` cap + `useVramFit` KV + loaded-model weights). | `run_context_cliff` / `stop_context_cliff` / `get_cliff_results` | `cliffStore` |
| **RunRecoveryDialog** | Resume/Discard an interrupted batch | `check_unfinished_run` / `resume_batch_eval` / `discard_run` | `batchStore` |
| **PipelinePanel** *(standalone)* | Single-task Config→System Pkg→Stream→Verify stepper | `trace_toolcall_task` / `load_toolcall_trace` | `evalRegistryStore` |
| **MatrixPanel / MatrixGrid** *(standalone)* | Tasks×models P/T/A grid + regression timeline | `run_collection_matrix` / `load_collection_history` | `evalRegistryStore` |
| **ToolCallPanel** *(standalone)* | Batch scoreboard + bar chart | `run_toolcall_eval` | `batchStore`, `evalRegistryStore` |

> **Hosting note.** The *live* Eval-tab tree is `EvalPage → EvalManager +
> MatrixScoreboard + TraceDebugger + PerformanceMatrix`. `ContextCliffPanel`
> lives on the **Audit** tab (`features/audit/AuditPage`). `PipelinePanel`,
> `MatrixPanel`/`MatrixGrid`, `ToolCallPanel`,
> `TaskDetailView` are self-contained composables (alternate/legacy surfaces over
> the same IPC) not wired into the current `EvalPage`; they remain documented as
> the canonical single-task/matrix/agentic renderers.

### Full IPC surface (`shared/ipc/eval/*`)

| Wrapper | Command | Args → Returns |
|---|---|---|
| `runBatchEval` / `stopBatchEval` (`batch.ts`) | `run_batch_eval` / `stop_batch_eval` | `(collectionId, targets, tasks, k?, maxSteps?, params?, keepAlive?, runNativeFc?)` → `BatchReport` |
| `runContextCliff` / `stopContextCliff` (`cliff.ts`) | `run_context_cliff` / `stop_context_cliff` | `(model, backend, collectionId, tasks, source, maxTokens, steps, params?, runId, modelPath?, runNativeFc?)` → `CliffReport` |
| `saveCliffResult` / `getCliffResults` | `save_cliff_result` / `get_cliff_results` | → `void` / `Record<model, CliffStatus>` |
| `runEvalTask` / `listEvals` (`evals.ts`) | `run_eval_task` / `list_evals` | `(taskId, model, backend)` → `EvalRunResult` |
| `runCollectionMatrix` / `loadCollectionHistory` (`matrix.ts`) | `run_collection_matrix` / `load_collection_history` | → `MatrixReport` / `RunSummary[]` |
| `checkUnfinishedRun` / `resumeBatchEval` / `discardRun` (`queue.ts`) | `check_unfinished_run` / `resume_batch_eval` / `discard_run` | → `UnfinishedRun?` / `BatchReport` / `void` |
| registry CRUD (`registry.ts`) | `list_builtin_collections`, `get_builtin_collection`, `list_custom_collections`, `load_custom_collection`, `save_custom_collection`, `delete_custom_collection`, `import_custom_collection`, `read_text_capped` | collections + tasks |
| `runToolcallEval` / `traceToolcallTask` / `loadToolcallTrace` (`toolcall.ts`) | `run_toolcall_eval` / `trace_toolcall_task` / `load_toolcall_trace` | → `ToolCallReport` / `TraceResult` |
| `assessReadiness` etc. (`readiness.ts`) | `assess_readiness`, profile CRUD | → consumed by readiness, see cross-link |

**Tauri event channels** (push, from `batch.ts` / `cliff.ts`):
`batch-progress` (`BatchProgress`), `agentic-step` (`AgenticStepPayload`),
`batch-complete` (`{report}`), `cliff-progress` (`CliffProgress`),
`cliff-step` (`CliffStep` — per-task sub-rung progress for the live line + ETA).

---

## `components/` — top-level

### EvalPage.tsx — the page

**Responsibility.** Owns the Eval-tab two-column layout and the cross-cutting
**context-shift cancellation law**. **Why it matters:** it is the single place
that decides what "the current eval context" is (backend × collection × model)
and halts everything when that context changes.

**What.** Left column (360px): `EvalManager`. Right column: in **run mode**
`MatrixScoreboard` (over `TraceDebugger`) + `PerformanceMatrix`; in **edit mode**
`CollectionEditor`. It also mounts `RunRecoveryDialog` (via `useRunRecovery`).

**How / where used.** A backend OR collection switch invalidates a **completed**
run's context, so `haltOldContext` clears a stale batch report — but **only when
idle**. The overriding **nav-persistence law**: a selection change must NEVER
cancel or detach an **in-flight** run. The backend/model selection is *global*
(it also changes from the Workspace picker), so a running batch keeps streaming,
and the results view stays bound to the RUN's model + live task:

```tsx
const haltOldContext = () => {
  if (!useBatchStore.getState().running) useBatchStore.getState().reset();
};
// Each detaching action is skipped while a batch runs, or the live Scoreboard/Trace
// would blank the moment the user switches screen (global model/backend) or tier.
useEffect(() => { haltOldContext(); if (!running()) setFocusedModel(""); }, [selectedBackend]);
useEffect(() => { haltOldContext(); if (!running()) setFocusedTaskId(null); }, [selectedCollection]);
useEffect(() => { if (running()) return; /* else re-target collection to the tier */ }, [effectiveTier, presets]);
```

The eval runs **one** model, kept a valid member of the *global* selection
(`selectedModelStore`) — no per-page picker. `focusedModel`/`focusedTaskId` drive
which (model,task) the scoreboard/trace inspect; while running, an auto-follow
effect points them at the live task so each task's trace shows AS it runs. Because
those focus writes and the tier→collection re-target are all gated on `!running`,
switching screen or difficulty tier mid-run no longer strands the live view (the
run's steps are keyed to the *starting* collection's task ids, so swapping the
selected collection out from under it would blank the trace). Clicking a matrix
row scrolls the detail panels into view.

### TraceDebugger.tsx — the live single-(model,task) inspector ("Evaluator")

**Responsibility.** The replayable pipeline for **one** cell of the batch.
Reads `outcomeByKey[cellKey(model,taskId)]` and `stepsByKey[...]` from
`batchStore` and renders `ConfigPhase` (prompt + tools) → `SystemMessagePhase`
(system pkg + user prompt) → Stream → `VerifyPhase` (pass/fail diagnosis), plus
an **inline agentic step timeline** for multi-step outcomes. Uses `traceDiag`
(from `verdict.ts`) for the failure explanation. Tabs + collapse toggle.
Exposes pure helpers `isErrorKind` / `getStepTitle` / `verdictLabel`: a step's
`kind` decides red-vs-green (`turn_timeout` + `forbidden_call` are failures, never
a green "success" card), and the failing-run header reads the report's actual
`top_error` (Malformed JSON / Hallucinated / Turn Timeout / Forbidden / Step Budget)
instead of a hardcoded "sequence violation".

**D9 budget diagnostic (`BudgetDiagnostic`).** On a `truncated` or `reasoning_overrun`
step (both carry `reasoning_tokens` / `context_used` / `context_window` from the backend),
the card renders TWO bars — thinking-budget and context-window fill — and a plain-language
cause + fix: **Reasoning-overrun** ("spent its whole thinking budget with memory to spare →
raise the preset", a *setting*) vs **Truncated (context-bound)** ("the window filled → a
*hardware* limit, bigger machine"). This keeps a budget problem from ever reading as
out-of-memory. `FailureTaxonomy` splits the two counts for the same reason.

**Pass^k run grouping.** `stepsByKey` holds every run's `TrajectoryStep`s in one
flat array (each agentic task runs k times; `step_index` restarts at 0 per run).
The timeline splits them with the pure helper `groupStepsByRun(steps)` and renders
each run as a **collapsible "Run N of K" section** with a PASS/FAIL/RUNNING chip
(`runPassed` — terminal step is `end_state_reached`; the last group reads RUNNING
while `running` since runs execute sequentially) and its own per-run turn numbering.
The run header also shows a **🧠 N thinking** badge — `runThinkingTokens` sums the
per-turn measured `reasoning_tokens` across the run (a reasoning model only; hidden at
0, never a fabricated N/A) so the user sees how much the model thought to reach the result.
Default-expanded run = the first completed-and-failed run, else the first; user
toggles are tracked per `run_index` and reset on task/model change. Without this,
k single-step runs rendered as ambiguous duplicate "TURN 1 / Sandbox Response
Injection" cards.

**Visual environment replay (split-view).** When a run's steps carry an `EnvView`
(`hasEnvReplay(steps)` — i.e. an environment task like the filesystem env), the expanded
timeline becomes a 2-column split: the text trace (left) beside a visual replay (right) in
`components/replay/`. `EnvironmentReplayPanel` holds a `StepScrubber` (◀/▶ + range over the
run's turns) and a kind-switched panel; it follows the live tail (defaults to the latest real
filesystem action, not a terminal no-op) until the user scrubs, then pins. `FileTreeReplay`
draws the file tree with the touched path highlighted and the **real returned content/matches**
(making the acks-empty fix visible — the user watches the model open a file and read it).
Non-environment tasks stream `env.kind === "none"` → no panel, zero change to the text trace.
`EnvView` is local-only (never published). Tested in `__tests__/EnvironmentReplay.test.tsx`.

**Per-run Input/Output drill-down.** Every run in the trace carries an **Input** and
an **Output** button (single-turn → on the Turn-1 card; agentic → on each "Run N of
K" header, `stopPropagation` so they don't toggle the run's expand). They open
`RunIoModal` scoped to that run: **Input** = the prompt the model was given (single →
captured `trace.system_message` + `user_prompt`; agentic → the system package
reconstructed from `task.tools` (unaffected by per-run id-randomization — tool schemas
carry no entity ids) + the user line, which prefers the REAL per-run prompt
(`TrajectoryStep.initial_prompt` on that run's step 0) over the static `task.prompt`
template when a run has actually streamed — a generated task re-randomizes its entity
ids per Pass^k run, so the template is only right for the one seed that happens to
match); **Output** = the raw response (single → `trace.raw_output`;
agentic → *only that run's* turns — `steps` filtered by `run_index` — incl. each
turn's sandbox injection and an `(empty output)` fallback for a whitespace turn). The
"no response" cases are surfaced explicitly, never a blank, each with its own testid:
`run-io-not-run`, `run-io-error` (shows the message), `run-io-empty`. While an
agentic run is still streaming (outcome not yet cached) the Output shows the in-flight
trajectory rather than a false "not run". `decoys` threads from `EvalPage` so a
reconstructed agentic Input admits the decoy tools the model also saw.

The view-model logic is the pure **`components/runIo.ts`** (`buildRunInput(task,
outcome, decoys?, steps?)` / `buildRunOutput(outcome, steps)`) — no React, encoding the
no-response branches and the fidelity **`note`** (non-null whenever the shown Input
isn't verbatim: a not-yet-run/**errored** single-turn task, or a decoy-widened agentic
run). `steps` is optional and, when the caller has it (`RunIoModal` always does —
`TraceDebugger` computes the same `initial_prompt` lookup locally for its own System
Prompt Pkg tab), the real prompt silently wins with no `note` — the fixed reconstruction
was never "approximate on purpose", just the best available before a run streamed.
`RunIoModal.tsx` is the thin renderer. The agentic system-package string lives once in
**`agenticPrompt.ts`** (`agenticSystemPreview`), imported by both `runIo` and
`TraceDebugger`, so the two can't diverge. Tested in `__tests__/runIo.test.ts` (unit)
and `__tests__/TraceDebuggerRunIo.test.tsx` (rendered).

### ToolCallPanel / ContextCliffPanel / CpuFallbackBanner / RunRecoveryDialog

| File | One-line |
|---|---|
| `ToolCallPanel.tsx` *(standalone)* | Batch scoreboard (task table + bar chart + stats + run controls) over `run_toolcall_eval`; `batchStore` + `evalRegistryStore` + `installedModelsStore`. |
| `CpuFallbackBanner.tsx` | Warns when Ollama weights spill to CPU; reads `loadedModels()` + hardware snapshot. |
| `RunRecoveryDialog.tsx` | Modal "Resume interrupted evaluation?" — Resume (keeps data) / Discard (destructive) / Esc-dismiss. Renders `run.collection_id` + `done/total`. |
| `ContextCliffPanel.tsx` | The full Context Stress Test — see [Context Stress Test](#contextcliff-folder). |

---

## `hooks/` — the run engines

### useBatchRun.ts — the batch event loop

**Responsibility.** Drives one batch over a **single** Tauri event stream.
Subscribes ONCE (on mount) to all three batch channels, validates each payload
with a Zod schema (drift is logged, never crashes), and routes into the
rAF-buffered `batchStore`. Also flushes the store if the installed-model *set*
changes (but never mid-run).

**Why.** Centralizes the subscription so no component double-listens, and
pre-flights every backend the run uses so a down server fails fast.

```ts
listen(EVENT_BATCH_PROGRESS, (e) => {
  const r = BatchProgressSchema.safeParse(e.payload);
  if (r.success) useBatchStore.getState().ingestProgress(r.data);
  else console.error("IPC payload drift (batch-progress):", r.error.issues, e.payload);
});
// run(): probe EVERY backend, then start
for (const backend of backends) {
  if (!(await healthFor(backend).catch(() => ({available:false}))).available) {
    useBatchStore.getState().setError(`${label} server isn't reachable …`); return;
  }
}
useBatchStore.getState().startRun();
await runBatchEval(collectionId, targets, tasks, k, maxSteps, globalParams,
                   keepLoaded ? -1 : undefined, runNativeFc);
```

`keepLoaded → keepAlive -1` keeps weights resident; off omits it (backend
default).

**`stop()`.** Calls `useBatchStore.getState().beginStop()` **synchronously before**
`await stopBatchEval()` — the store (and therefore the Stop button) reflects the
click instantly, instead of waiting on the IPC round-trip. The cancel token is
checked at every layer down to the in-flight HTTP call itself (turn boundary,
post-model-call, and inside `stream_generate`'s own request — see
`backend-eval-engine.md#cancellation`), so in practice Stop lands within a few
hundred ms. `stopping` stays true until that lands — the flag exists so the wait,
however short, is always visible rather than the button looking unresponsive.

### useEvalRun.ts — sequential single-model runner

**Responsibility.** Run every loaded task **sequentially** against one model via
`run_eval_task`, recording each into `evalStore`. Stops on the first IPC error and
surfaces it — never fabricates a score for a task that didn't run. (Simpler path,
used by the legacy single-model surfaces; the batch path is the live one.)

### useRunRecovery.ts — crash recovery

**Responsibility.** On mount, ask the backend `check_unfinished_run`; if a run was
interrupted (and nothing is currently running), surface it for a Resume/Discard
prompt. **Resume** calls `startRun()` then `resume_batch_eval(runId)` — the
backend bulk-paints the already-done Matrix cells then streams the live tail
through the *same* batch events; `await` settles on the final report. **Discard**
drops the recovery log; **Dismiss** keeps it for next launch.

```ts
const resume = useCallback(async () => {
  const runId = pending.run_id; setPending(null);
  useBatchStore.getState().startRun();
  try { await resumeBatchEval(runId); }
  catch (e) { useBatchStore.getState().setError(formatIpcError(e)); }
}, [pending]);
```

---

## `state/` — the four stores

### batchStore.ts — the throttled batch sink

**Responsibility.** Absorbs the high-frequency batch event stream without a
per-event render. **Why:** an agentic model emits hundreds of `agentic-step`
events/sec; naïve `set()` per event would thrash React.

**Shape.** `report` (heavy per-model Matrix, null until `batch-complete`),
`outcomeByKey` (terminal `TaskOutcome` per `cellKey(model,taskId)`), `stepsByKey`
(live `TrajectoryStep[]` per cell), `tasksByModel`, `collectionId` (the run's
collection, read off the stamped `collection_id` on the events — the Latency page's
Test-run view binds to it), `progress {done,total}`, `flushes`, `error`, `running`,
`stopping`.

**taskCost.ts.** Pure derivation over one cell's `TrajectoryStep[]` → `TaskCost`:
prefill/eval ms + token sums over all Pass^k runs, `cacheHitTokensTotal` (llama.cpp
`cache_n` only — null on Ollama/MLX renders "Not available", never 0),
`kvTokensMeasured` (picks the KV honesty tier: "computed from measured tokens" vs
"estimated"), `peakContextTokens` (max single-run occupancy — sizes the KV headline),
and `maxStepEndRssBytes` (max of step-END samples — NOT a true in-step peak, and
whole-process, never a per-task delta). Every field null when nothing reported it.

**`stopping`.** True from `beginStop()` until the run actually ends (`complete(...,
final=true)` or `setError`) — a THIRD state alongside `running`, not a rename of it.
`running` alone can't tell "actively executing" apart from "cancel requested,
draining the current turn"; the Stop button reads `stopping` to switch to a
disabled **Stopping…** state the instant it's clicked, rather than staying on an
actionable-looking STOP BATCH until the backend's `batch-complete` lands. An
intermediate complete (`final=false`, e.g. the native pass ending before the prompt
pass) leaves `stopping` untouched — only the FINAL complete clears it, so the button
doesn't flicker back to normal mid-stop.

**How.** Two guards. (1) A **module-level rAF buffer**: events accumulate and
flush at most once per animation frame. (2) An **event gate** (`accepting`): only
true between `startRun()` and `reset()` — late events from an *abandoned* run
(collection/backend switch) are dropped, so the cleared store never re-pollutes
with the wrong collection's data. The gate is decoupled from `running` so a
resume can keep streaming after a partial complete.

```ts
let accepting = false;                 // event gate
ingestStep: (p) => { if (!accepting) return; buffer.push({t:"step",p}); scheduleFlush(); },
complete: (report) => { if (!accepting) return; flushBuffer(); set({report, running:false}); },
reset: () => { accepting = false; buffer = []; if (frame) unschedule(frame); set({...empty(), flushes:0}); },
// applyBatch folds the buffer: progress→outcomeByKey/tasksByModel, step→stepsByKey
```

`cellKey(model,taskId)` is the NUL-joined cache key shared by every reader.

### cliffStore.ts — the survivable probe

**Responsibility.** Runs the Context Stress Test and holds its live series so the
run **survives tab navigation**. Also caches backend-persisted context limits per
(collection, model) for the Model Results.

**Shape.** `request` (Matrix pre-fill, consumed by the panel), `points`
(`CliffPoint[]`, live), `running`/`runningModel`, `progress` (per-rung done/total),
`step` (latest fine-grained `CliffStep` — per task generation — drives the live
"rung r/N · position p/3 · task t/M" line + ETA so a slow deep rung never looks
stuck), `startedAt` (ms, for the ETA), and three
collection→model maps: `results` (genuine **collapse depths only**), `probed`
(completed this session, even when no cliff — distinguishes "probed healthy" from
"unprobed"), `brokenBaseline` (failed at the *smallest* context — a tool-call
failure, NOT a context limit).

**How.** A **module-level generation token** (`activeRun`) makes a long sweep
cancellable and re-run-safe; `cliff-progress` events are filtered by both the
token AND the `run_id`, so two runs of the *same* model never bleed. A second
listener on `cliff-step` (same filter) updates `step` per task generation so the
panel's progress bar/ETA advance *within* a rung, not only between rungs. Guardrails:
(1) the probe is **never auto-run** — the user clicks Execute; (2) state is
cleared *before* dispatch so a re-run never appends to a stale series. The awaited
`report` is authoritative — it *replaces* the live series so chart and persisted
status can never disagree.

```ts
unlisten = await listen<CliffProgress>(EVENT_CLIFF_PROGRESS, (ev) => {
  const p = ev.payload;
  if (activeRun !== myRun || p.run_id !== myRun) return;   // token + run_id filter
  set((s) => ({ points: [...s.points,
    { promptTokens: p.point.verified_tokens || null, composite: p.point.composite, trace: p.point.trace }],
    progress: { done: p.done, total: p.total } }));
});
const report = await runContextCliff(model, backend, collectionId, tasks, source, maxTokens, steps, params, myRun);
// stop(): activeRun++ then stop_context_cliff() — actually cancels the backend, not just the UI.
```

`hydrate(collectionId)` reads `get_cliff_results` → restores `results` /
`probed` / `brokenBaseline` so states survive a reload.

### evalRegistryStore.ts — collections & active selection

**Responsibility.** Holds the available datasets (read-only built-in **presets** +
user **custom collections**) and the active `selected` + its `tasks`. The runner
is always handed `tasks`.

**How.** `init` loads `list_builtin_collections` + `list_custom_collections` in
parallel, **publishes the presets to the picker first**, then loads the
`DEFAULT_PRESET = "easy-coding"` tasks — so a single failing default-collection
load can't blank the whole Built-in list (a silent init failure previously left
the page stuck on "Custom JSON" with no collections); the error surfaces in the
panel's error banner instead of being swallowed. `listBuiltinCollections` also
parses **per row** (`safeParse` + drop-and-`warn`), so one malformed bundled
collection — e.g. a future scenario with an unknown tier — drops out with a warning
rather than throwing the whole array and emptying the picker + tier list together. NOTE: the registry Zod mirror
(`registry.ts`) must track the backend `EndStateRule` exactly — every bundled v2
scenario serializes `{ require_all: [...] }`, and the spec's v2-only keys
(`world_state`, `must_not_call`, `name_faults`, `generated`) must survive the parse,
since the parsed tasks are handed straight back to `run_batch_eval`. `select`
dispatches on `isPreset` (`get_builtin_collection` vs `load_custom_collection`).
`save`/`remove`
call the registry CRUD then re-list. Presets can't be deleted from disk, so
`hidePreset` just hides them (persisted in `localStorage` under
`qm-eval-hidden-presets`). `startNew()` enters the `NEW_COLLECTION = "__new__"`
sentinel (never sent to the backend).

`importFile` is **two-phase**: it dry-runs `validate_collection_file` on the picked
path first and only imports (`import_custom_collection`) a clean file — a failing
`CollectionValidation` verdict is returned to the caller **without writing anything**,
so the UI shows the structured per-task findings instead of a raw backend error
string (the backend `evals::save` boundary would reject it anyway; this is the
friendly layer). `save` auto-validates what was just written
(`validate_custom_collection`, best-effort) and returns the verdict so the author
sees "answer keys valid" without pressing Validate.

In **EvalManager**, "[↓] Import .json" opens a **format-guide dialog first** (the
expected v2 skeleton + copy-template button + a pointer to
`docs/reference.md#agentic-authoring-contract`); the file picker only opens from
its Continue. A blocked import shows a popup listing each finding (task id + exact
defect) and mirrors them in the validation panel (red `✗` rows,
`eval-validation-semantic-*` testids). An `ok` verdict that carries
**answer-grounding warnings** (`semantic_warnings` — the heuristic
`UngroundedAnswerToken` check, with its "checked the prompt, N tool names, M data
blobs" evidence) imports fine: the store returns the verdict, a toast points at the
panel, and the warnings render as amber `⚠` rows (`eval-validation-warning-*`) — the
author judges each one; a heuristic never blocks.

### evalStore.ts — simple per-task results (legacy single-model)

`tasks`, `results: Record<taskId, EvalRunResult>`, `running`, `currentId`,
`error`. `setResult` merges by `task_id`; `passRate(results)` aggregates. Used by
`useEvalRun`; the live batch path uses `batchStore` instead.

---

## `components/pipeline/` — single-task phase view

`PipelinePanel` is the standalone Config→System Pkg→Stream→Verify stepper over
`trace_toolcall_task` (run live) / `load_toolcall_trace` (replay a cached run
without re-inference). The four phases are dumb presentation components; the
live `TraceDebugger` reuses `ConfigPhase`/`SystemMessagePhase`/`VerifyPhase`.

| File | Phase | Renders |
|---|---|---|
| `ConfigPhase.tsx` | 1 · Input Config | task `prompt` + `JSON.stringify(task.tools)` |
| `SystemMessagePhase.tsx` | 2 · System Pkg | the assembled `system_message` + `user_prompt` |
| `StreamPhase.tsx` | 3 · Stream | the model's **real** `raw_output` (terminal view + caret while running) |
| `VerifyPhase.tsx` | 4 · Verify | pass/fail verdict for the task category |
| `pipelineStyles.ts` | — | shared `panelBox`/`panelLabel`/`codeBlock` styles |

**Phase progression** (`PipelinePanel`):

```tsx
const PHASES = ["Input Config", "System Pkg", "Stream", "Verify"] as const;
// ▶ handleRun → traceToolcallTask(model, backend, task) → setTrace; phase stepper ‹ / ›
{phase === 0 && (task ? <ConfigPhase task={task}/> : …)}
{phase === 2 && (trace || running ? <StreamPhase output={trace?.raw_output ?? ""} running={running}/> : needsTrace)}
{phase === 3 && (trace ? <VerifyPhase verdict={trace.verdict} category={task?.category ?? "single"}/> : needsTrace)}
```

A Scoreboard **handoff** (`focus = {collection, taskId, model}`) jumps to that
cell and shows the *cached* trace without re-running. `execState`
Idle/Running/Cached/Complete; `validation` PASSED/FAILED (via `isPassed`).

---

## `components/scoreboard/` — the comparison surface

### scoreRows.ts — the BatchReport → row transform

**Responsibility.** The single transform from a `BatchReport` to display rows.
**Why it matters:** it encodes the "never fabricate a metric" rule — null sources
render `"—"` (inapplicable, e.g. single-turn has no steps) or `"N/A"` (not
measured, e.g. native FC unsupported), never `0`.

```ts
export function toScoreRows(report, models): ScoreRow[] {
  return report.columns.map((c) => {
    const ag = c.agentic;
    const pass = c.error ? "Error"
      : ag ? `${ag.tasks_passed}/${ag.tasks_total}`   // strict Pass^k
           : fmtPct(c.toolcall?.composite);            // single-turn composite %
    const nat = c.agentic_native_fc;
    const passKNative = c.error ? "Error" : nat ? `${nat.tasks_passed}/${nat.tasks_total}` : "N/A";
    return { model: c.model, label: modelLabel(info ?? {name:c.model}), quant: info?.quantization || "—",
      passK: pass, passKNative,
      avgSteps: ag ? fmtNum(ag.avg_steps) : "—",
      effort:   ag ? fmtTokens(ag.avg_output_tokens_success) : "—",
      schemaResil: ag ? fmtPct(ag.schema_resilience) : "—",
      topError: c.error ? "Error" : ag ? TOP_ERROR_LABEL[ag.top_error] : "—",
      failures: ag?.failures ?? null, composite: fmtPct(c.toolcall?.composite) };
  });
}
```

### PerformanceMatrix.tsx — the cross-model table ("4. Model Results")

**Responsibility.** One row per model with the full metric set, and the **bridge
to the cliff probe**. Reads `batchStore.report` → `toScoreRows`, and the cliff
caches from `cliffStore` (hydrated on mount per `report.collection_id`).

**How.** Per-model badges via `getPassKBadge`/`getSchemaResilBadge`/
`getTopErrorBadge` (green = perfect, amber = partial, red = failure). The **Context
Limit** cell is a small state machine, checked in order:

1. `probing…` (this model is the running probe)
2. **`fails from start`** (red) — `brokenBaseline` (checked *before* a depth: a
   broken baseline is persisted as a depth for the readiness gate, but the Model Results
   must show the failure, not dress it as a cliff)
3. **`{n} tok`** — a genuine measured collapse depth (`results[model]`)
4. **`✓ no cliff`** (green) — `probed` healthy across the range
5. **`Run probe ↗`** — unmeasured; `reprobe(model)` sets `cliffStore.setRequest`
   and navigates to the **Audit** tab (never auto-runs — guardrail 1)

`Native FC` column is behind a toggle (`showNative`); N/A is explained (only
Ollama models whose `/api/show` lists the `tools` capability). The `ⓘ` on Top
Error portals the full failure breakdown (Loop Cap / Fake Done / Bad Schema /
Malformed). Clicking a row → `onFocusModel` (scrolls the detail panels up).

**Schema-Resil. three states** (`getSchemaResilBadge`). Schema resilience is a
recovery rate `recovered / hit` (backend `report.rs`), so it is `null` when **no
call ever failed schema validation** — a *good* result, not a missing one. To keep
that legible without a hover, the badge renders **`✓ clean`** (green chip + tooltip)
when the run happened and hit zero schema errors (`schemaResilNote` is set), a real
**percent** chip when there were errors to recover from, and only a **bare `—`** when
the metric was genuinely *not measured* (single-turn / errored column — no note). The
`✓ clean` label is deliberately **not** a fabricated `100%`: there was no denominator,
so it says "clean", not a rate.

### MatrixScoreboard.tsx — the per-task table ("2. The Simulator")

Per the **focused** model: an `aggregate(byKey)` helper computes pass-rate / avg-steps /
effort over `tasks × byKey`, called **once per pass** — `nativeOutcomeByKey` and
`outcomeByKey` — and the AGGREGATE box renders **one labeled line per measured pass**
(`Tool-Calling (native):` / `Prompt-based:`). The two are **never blended** (different
measurement modes — eval-metric comparability); a native-only run shows the native line
(previously the AGGREGATE read only the prompt pass and showed `—` for a native run). A
live progress bar (`progress.done/total`) and a task table follow. Each row's Result badge:
`single` → Pass/Fail; `agentic` → all-pass `Pass`, none `Fail`, **partial → amber
`Partial p/total`** ("unreliable, not a clean pass"). Row click sets `focusedTaskId` →
drives `TraceDebugger`. Collapsible.
Read-only — per-task Edit/Delete live in the sidebar (`EvalManager`), not here. Header
chips echo the run shape (`Tier · K · Decoys`, plus `Task: <id>` when a single task is
scoped). When the `runTaskId` prop is set, the whole board (rows **and** the aggregate)
filters to that one task via `collectionTasks.filter` — a stale id not in the collection
falls back to the full board rather than rendering blank.

---

## `components/matrix/` — the standalone matrix surface

`MatrixPanel` owns its own collection picker + multi-select `ModelDropdown`, calls
`run_collection_matrix` + `load_collection_history`, and toggles between
`MatrixGrid` (tasks×models P/T/A badge grid; `onViewTrace` cells) and
`HistoryTimeline` (SVG regression line of composite score over past runs, one line
per model). All pure-presentation given the report.

| File | Role |
|---|---|
| `MatrixPanel.tsx` | Run + view toggle; `evalRegistryStore` + `installedModelsStore`. |
| `MatrixGrid.tsx` | Tasks×models grid; cell = unrun `—` / scored badge (P/T/A/Abs pills) / clickable. |
| `HistoryTimeline.tsx` | SVG composite-over-runs regression chart. Each model spans the plot on its **own** run ordinal (oldest→newest), so models with fewer runs aren't left-packed against the longest one and a single-run series sits at the left edge. The y-scale clamps to [0,1]; runs with neither composite nor Pass^k are counted and noted under the chart. |
| `ModelDropdown.tsx` | Multi-select dropdown of matrix columns (Set + onToggle). |

The **Audit tab** (`features/audit/AuditPage`) reuses `HistoryTimeline` for the saved
regression history. Its collection picker uses the tier-grouped `PresetOptGroups` (not a
flat list) so same-domain scenarios across tiers — `easy/medium/hard-coding` all humanize
to "Coding" — stay distinguishable under Easy/Medium/Hard/Extreme `<optgroup>`s. A history
load is never swallowed: a failure shows an error banner, runs recorded only under a
*different* backend show an explicit "runs under other backends" note, and only a genuine
absence falls through to the timeline's "No run history yet" empty state.

---

## `components/manager/` — collections & authoring

### EvalManager.tsx — the run + collections control hub (left column)

**Responsibility.** Every run control plus the entry points to authoring/import.
Sidebar order (top→bottom): **Model → Difficulty Tier → Collections → Iterations →
Max Steps → Anti-Saturation → (Native-FC, RUN BATCH, Export)**. Calls **`useBatchRun`**.
The built-in collection list is **filtered to the chosen tier** (the data-source toggle,
now inside the Collections section, still switches built-in/custom). **Clicking a collection** expands/collapses its task list beneath it (accordion,
`expandedId` state; click also `select`s it; `collectionRow` → `renderTasks`, shown when
`expandedId === selected`).
Each task row is **click-to-select** (scopes the run to that one task — see below) and
reveals **Edit** (`onEditTask`) + **Delete** (`onDeleteTask`) on hover — wired from `EvalPage`
(this replaced the scoreboard buttons and the old collection-level "Edit"). The Edit/Delete
buttons `stopPropagation` so they never toggle the selection.
**+ New Collection** + **Import JSON/CSV** sit at the end of the collection list; Export
is at the bottom. The Decoy control carries an `InfoButton` (`TOOL_HELP.decoys`). Run is
disabled without a model + tasks. Delete-collection differs: presets are *hidden*
(`hidePreset`), customs are *removed*.

**Run/Stop button, three visual states.** Idle → **▶ RUN BATCH**. Running
(`batchStore.running`) → red **■ STOP BATCH** (clickable — same button toggles
Run↔Stop via `handleRunBatch`). Stopping (`batchStore.stopping`, set the instant
Stop is clicked) → muted, disabled **■ STOPPING…** with a "finishing the current
step" hint, so the click has immediate visible feedback even though the backend
may take a few seconds to actually halt (see `useBatchRun.ts` above).

**Single-task runs (Built-In · Custom · MCP).** Clicking a task row in the
sidebar **selects just that task** (`runTaskId`, owned by `EvalPage`; ◉/◯ marker,
click again to clear back to the whole collection). With one task scoped:
- the button reads **▶ RUN TASK** / **■ STOP TASK** (`singleTaskScope`);
- `handleRunBatch` filters `runTasks` to `[the one task]` — the backend runs
  exactly the tasks it's handed (`batch_cmd.rs` uses the passed `tasks`, not a
  reload from `collection_id`), so **all other levers (k, Max Steps, decoys,
  thinking budget, global params, model, native/prompt method) apply unchanged**;
- the **Simulator** scopes its rows + aggregate to that task (`MatrixScoreboard`'s
  `runTaskId` prop), the **Evaluator** previews it (`EvalPage` sets `focusedTaskId`),
  and **Model Results** naturally shows the single-task summary (it's derived from
  the `report`, which now covers only that task);
- the CLI-equivalent preview is replaced by an honest note — `qm run`/`qm test`
  run whole collections, so a single-task run is a UI-only convenience, never a
  faked whole-collection command.
MCP already scopes by its own highlighted task (`selectedMcpTask`); the button
rename covers it too. A single-task selection is dropped when the collection or
data-source changes (a different collection has different task ids). Because only
a subset of the collection ran, `verified_collection_hash` returns `None`, so a
single-task run is **not publishable** — correct: it isn't the whole collection.

**The k pre-fill guard (EvalPage owns it).** `k` is always editable and pre-filled
with the chosen tier's `PASS_K_BY_TIER` recommendation, but the pre-fill is a
**programmatic write that must never clobber a value the user fixed**. A synchronous
`suppressAutoK` **ref** is set the instant the user fixes k — by typing it
(`setIterationsKByUser`) or by picking a concrete tier. The only async write —
`Auto`'s recommended k landing when the `getHardwareTier` probe resolves — is keyed
on `[hwTier]` and skips when `suppressAutoK` is set. Because a ref updates
synchronously (not subject to render/effect ordering), this holds **even when the
hardware probe resolves in the same React flush as a tier change** (the effect would
otherwise run with a stale `tierSel`). The editable `k` is always sent to the run and
wins over the tier policy in the backend's `apply_overrides`.

### CollectionEditor.tsx — the authoring surface (center, edit mode)

Dual view: `TaskListView` (the list) ↔ `TaskSandboxConfigurator` (edit one task).
Holds a `TaskDraft[]` mirror of the active collection, validates on save via
`validateDrafts` (`evalDraft.ts`), and persists through `evalRegistryStore.save`.
Editing a **preset** forces a name (NameDialog) → saves a new custom copy.
ConfirmDialog guards destructive steps. Accepts an **`initialTaskId`** so a
scoreboard-row "Edit" lands directly in that task's configurator (not the list).
The scoreboard's per-task **Delete** is handled in `EvalPage` instead (confirm →
`save` the filtered tasks; a built-in forks to an auto-named custom copy).

### CsvImportModal.tsx — CSV → tasks

**Responsibility.** A live-validated single-turn importer. The shared **Tools box**
supplies tool schemas once; the CSV carries per-case data only
(`id,prompt,expected_tool,expected_args`). Picks a file via the OS picker →
`read_text_capped` (Rust reads + size-caps; the frontend never reads arbitrary
files). Parses live with `csvToCollection` (per-row ✓/✗); import is gated on
`result.tasks && name.trim()` so a partially-broken CSV can never be saved.

### TaskListView / TaskDetailView / TaskSandboxConfigurator / StatsBar / dialogs

| File | One-line |
|---|---|
| `TaskListView.tsx` | Compact task list + toolbar (Add/Save/Run-all); pass/fail badge per row; empty-state. |
| `TaskDetailView.tsx` | Single-task editor: id, category, prompt, tools/expected JSON + error line; StatsBar + verdict checklist. |
| `TaskSandboxConfigurator.tsx` | Edits a full task incl. agentic fields (mocks, end-state, faults, max-recovery); form state only. |
| `StatsBar.tsx` | The 4 tool-call sub-scores (parse / tool / args / abstain rates). |
| `NameDialog.tsx` | Prompt to name a new/forked collection. |
| `ConfirmDialog.tsx` | Generic destructive-action confirm. |
| `KebabMenu.tsx` | `⋯` overflow menu of actions. |

---

## Context Stress Test folder

### cliff.ts — verdict classification (pure)

**Responsibility.** Classify a completed probe series into a `CliffVerdict`. The
baseline (rung 0) must clear `CLIFF_BASELINE_PASS = 0.5` before any
"cliff"/"no-cliff" verdict is even considered — a model already failing at the
smallest context has no plateau to fall off.

```ts
export function classifyCliff(points, margin = 0.2): CliffVerdict {
  const base = points[0]?.composite;
  if (base == null) return { kind: "no-baseline" };
  if (base < CLIFF_BASELINE_PASS) return { kind: "broken-baseline", baseline: base };
  for (let i = 1; i < points.length; i++)
    if (points[i].composite != null && base - points[i].composite >= margin)
      return { kind: "cliff", depth: points[i].promptTokens };
  return { kind: "no-cliff" };
}
// cliffPoint() = thin wrapper → the depth, so persisted depth & verdict never disagree.
```

### ContextCliffPanel.tsx — the probe UI (Audit tab)

**Responsibility.** Pick a dataset + model + padding preset, set Max-Tokens and
Test-Steps ladders, run the probe, and graph where accuracy collapses. Owns its
own collection selection (independent of the editor). All probe state lives in
`cliffStore` so it survives navigation.

**How.** Consumes a Matrix pre-fill **reactively** (keyed on
`cliffStore.request`, not on mount — the always-mounted Audit page would miss a
mount-only effect) → sets model override + collection + tokens + steps, then
`consumeRequest()`. Max-Tokens defaults to — and its slider caps at —
`usableCliffTokens(context_length)` = the model's real context window (`useVramFit` dims,
`/api/show`) **minus `CLIFF_CTX_HEADROOM`** (both from `shared/ipc/eval/cliff.ts`, mirroring
the backend constant), and clamps on model switch. The headroom subtraction is load-bearing,
not cosmetic: the backend runs the probe at `maxTokens + CLIFF_CTX_HEADROOM`, so offering the
full window asked for more context than the model has — for every model — and Ollama answers
that by silently clamping and truncating the needle away, yielding a fabricated cliff at the
window. The rung table shows
each step's tokens / accuracy / Pass·Failure / **View trace** (expands the system
prompt + per-position model output, "needle at N%"). Read-out maps the verdict to
`≈Nk context tokens` / `broken baseline` / `accuracy maintained up to ≈Nk` /
`Idle`. A detected cliff whose collapse rung had no measured token count reads as
**"Cliff detected — context-token depth not reported"** — it never falls through to a
non-cliff message, and never substitutes a different rung's depth as if it were the
cliff's (no fake precision). Execute is greyed without a model + tasks; while running
it becomes Stop. The **Equivalent CLI command** preview (`CliCommandPreview` +
`buildCliffCommand`) sits at the **top of the panel, right under the collection
select** — the same placement as Run History's preview on the Audit page — so the
`qm cliff` command is visible before the run, live-updating with every control.

### ContextCliffChart.tsx — accuracy-vs-depth (visx)

SVG line chart (visx `scaleLinear` + `Group`): accuracy% (y) vs prompt-token
depth (x). Only rungs with **both** a measured token depth and an accuracy are
plotted (a rung with no `prompt_eval_count` is dropped, never placed at a
fabricated x) — and when any are dropped a caption below the chart says how many,
so the gap is visible rather than a silently shorter line. The y-scale is `clamp`ed
so a corrupt out-of-[0,1] composite pins to the axis edge instead of rendering
off-canvas. Draws a red dashed **Cliff Threshold** line at `cliffPoint(points)`,
an area fill, per-point dots (red past the cliff), and a hover tooltip
("≈N ctx tokens · X% accuracy · past cliff").

---

## Data-flow walkthroughs

### (a) Tool-call collection across models → Model Results → scoreboard

```
EvalManager: pick collection (evalRegistryStore.select → tasks), pick models, set k/maxSteps
  → useBatchRun.run(collectionId, targets, tasks, k, maxSteps, runNativeFc)
       ├ health-probe every backend (fail fast)
       ├ batchStore.startRun()  (gate open, buffer cleared)
       └ runBatchEval(...)  → IPC run_batch_eval
Backend streams: batch-progress (per task done) ─┐
                 agentic-step (per turn) ────────┤→ useBatchRun listeners → Zod-validate
                 batch-complete ({report}) ──────┘   → batchStore.ingest*/complete (rAF-buffered)
MatrixScoreboard: reads outcomeByKey for focusedModel → per-task Pass/Fail/Partial + live progress
PerformanceMatrix: on batch-complete reads report → toScoreRows → one row per model
  → click row → focusModel → TraceDebugger replays that cell (Config→System→Stream→Verify)
```

### (b) Agentic run → TraceDebugger timeline

```
A task with category "agentic" runs the sandbox loop; backend emits agentic-step events
  → batchStore.stepsByKey[cellKey(model,taskId)] accumulates TrajectoryStep[]
Scoreboard row Result = Pass^k (all k pass / partial p/total / fail)
  → click → TraceDebugger: Pass^k header + colour-coded turn timeline
    (tool_call · schema_error · hallucinated_completion · infinite_loop · end_state_reached)
    + per-turn llama.cpp prefix-cache readout (CacheBadge): green "N reused / M recomputed"
      when the prefix was reused, amber "⚠ CACHE BUST · M re-prefilled (+ms)" when a
      non-first turn's prefix collapsed (reuseRatio < CACHE_BUST_BELOW=0.5), neutral on the
      first turn; absent for Ollama/MLX. The bust is rare by design (agentic_num_ctx sizes
      the per-run window to keep the prefix cacheable) — so it flags a genuine anomaly.
```

### (c) Batch crash → recovery dialog → resume

```
App relaunch → EvalPage mounts → useRunRecovery → check_unfinished_run
  → UnfinishedRun {run_id, collection_id, done, total}  → RunRecoveryDialog
Resume → batchStore.startRun() → resume_batch_eval(runId)
  → backend bulk-paints already-done cells, then streams the live tail through the SAME
    batch events (gate stays open across the partial complete) → await → final report
Discard → discard_run(runId) (drops the log) ; Dismiss → keep for next launch
```

### (d) Context Stress Test → chart

```
PerformanceMatrix "Run probe ↗" → cliffStore.setRequest({model,backend,collectionId,maxTokens,steps})
  → nav to Audit tab → ContextCliffPanel consumes request (pre-fills, never auto-runs)
User clicks Execute → cliffStore.runProbe(...)
  ├ myRun = ++activeRun; clear points; running=true
  ├ listen(cliff-progress): filter by activeRun===myRun && run_id===myRun → append CliffPoint
  └ runContextCliff(...) → backend owns the ladder/padding/verify-and-adjust/persist
On resolve: report.points REPLACE the live series (authoritative)
  → classifyCliff → results[collection][model]=depth | brokenBaseline | probed
  → ContextCliffChart draws accuracy-vs-depth + red Cliff Threshold; hydrate() restores on reload
  → the depth feeds the model's Agent-Readiness verdict (see frontend-inspector-quant-agentreport.md)
```

---

## Support files

| File | Role |
|---|---|
| `verdict.ts` | Pass/fail scoring for tool-call results — `scoreLabel`, `isPassed`, `traceDiag` (failure diagnosis), `verdictToScores`, badge styles. |
| `evalDraft.ts` | `TaskDraft` validation/assembly — `draftFromTask`, `newDraft`, `validateDrafts` (Zod `ToolTaskSchema`/`ExpectedSchema`); shared by the form editor AND CSV import. |
| `csvImport.ts` | RFC-4180 `parseCsv` + `csvToCollection` (strict header/tools/per-row validation → `ToolTask[] | null`); delegates final assembly to `validateDrafts`. |
| `exportBatch.ts` | `batchToCsv` + client-side download (quote-aware `csvCell`). |
| `help.ts` | In-app help copy — `TOOL_HELP`, `METRIC_HELP`, `metricTitle`. |

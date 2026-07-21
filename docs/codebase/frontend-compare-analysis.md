# Frontend — Analysis (Compare) Tab

Documents the **Analysis tab**, the side-by-side model comparison feature, file by
file. Source root: `frontend/src/features/compare/`.

Cross-references:
- **Backend contract:** [`backend-compare.md`](./backend-compare.md) — the
  `run_compare`/`stop_compare`/`save_compare_report` commands and the per-model
  event stream this tab consumes.
- **App shell / nav / stores:** [`frontend-overview.md`](./frontend-overview.md).
- **Quant sub-tab** (lives inside this page):
  [`frontend-inspector-quant-agentreport.md`](./frontend-inspector-quant-agentreport.md).
- **Model picker source** (`useSelectedModelStore`, `useModelLabel`, installed
  models): [`frontend-models.md`](./frontend-models.md).

---

## Overview

**Why.** Local LLMs differ wildly in quality and speed at the same prompt. The
Analysis tab answers "given *this* prompt, which of *these* N models do I want?"
by running them **side by side** and showing outputs, throughput, and
time-to-first-token together — plus an up-front **hardware feasibility** gate so
you don't kick off a run your machine can't hold.

**What it shows.**
- One **output column per model** (`CompareColumn`), streaming live with a
  status badge (pending → loading → running → done/cancelled/error). Its done-state
  metrics line (`formatMetrics`) appends a llama.cpp **prefix-cache** segment
  (`cache N/total reused`) only when `cacheReuse(stats.cache_n, stats.prompt_eval_count)`
  is `available` — purely additive, so Ollama/MLX rows are byte-identical to before.
- A **metrics bar chart** (`MetricsChart`) — THROUGHPUT (tok/s) and TTFT (ms),
  ASCII bars with data-derived axis ticks and a pairwise diff caret.
- A **word-level token diff** (`CompareDiff` + `DiffView`) between exactly two
  finished outputs, via `diff-match-patch`.
- A **hardware summary + run-strategy picker** (in the Workspace, not the tab):
  per-strategy verdicts (OK / Risky / Won't fit) from `assessStrategies`.
- **Markdown / JSON export** of the whole run (`ExportButtons` + report builders).
- A **Quant sub-tab**, merged into this page (see cross-ref above).

**How (IPC + events).**
- Commands via `shared/ipc/compare/*`: `runCompare` → `run_compare`,
  `stopCompare` → `stop_compare`, `saveCompareReport` → `save_compare_report`,
  `getHardwareSnapshot` → `get_hardware_snapshot`.
- The backend streams **per-model row events** (`compare-loading`,
  `compare-token`, `compare-done`, `compare-cancelled`, `compare-error`) plus a
  terminal `compare-run-done`. `compareEventBus.ts` `listen()`s once, Zod-parses
  each payload, and patches the matching row in `compareStore`.

The tab is **read-only**: model selection and the Run trigger live in the global
header and the Workspace (`MultiRun`). It only renders the store's `rows`.

---

## Data-flow walkthrough

```
Workspace: author prompt + pick N models (selectedModelStore)
   │
   │ HardwareSummary reads get_hardware_snapshot → store.hardwareSnapshot
   │ assessStrategies(models, snapshot) → per-strategy OK/Risky/Won't-fit badges
   ▼
MultiRun "Compare (N)" → copies prompt/system into store → nav to "compare"
   │                                                       → useCompareRun.start()
   ▼
useCompareRun.start():
   guard: ≥1 model, non-empty prompt, chosen strategy not "wont_fit"
   initRun(models) → rows = [newRow per model], isRunning = true
   runCompare({ models, prompt, strategy, system?, params, perModelParams?,
                backends[], keepAlive? })  ─────────────► run_compare (Rust)
   ▼
backend streams, per model:
   compare-loading → setRowLoading   (pending → loading)
   compare-token   → appendToken     (→ running, output += text)
   compare-done    → setRowDone       (+ ttft_ms, tokens_per_sec, token_count, timeline, stats)
   compare-error / compare-cancelled → setRowError / setRowCancelled
   …after all models: compare-run-done → finishRun (isRunning = false)
   ▼
AnalysisTab renders rows:
   CompareColumn ×N  +  MetricsChart  +  CompareDiff (only if exactly 2 done)
   ▼
ExportButtons → buildReport(store+installed) → toMarkdown / toJson
             → save dialog → save_compare_report (Rust writes file)
```

---

## `components/` — UI

### AnalysisPage.tsx — sub-tab host (IMPORTANT)

**Responsibility.** Hosts two sub-tabs: **Analysis** (`AnalysisTab`) and
**Quant** (`QuantPage`, from `features/quant`).
**Why.** Quant is *one lens of analysis*, not a top-level nav item, so it was
merged in here. The active sub-tab is purely local `useState` — nothing
deep-links into it.
**What/How.** Renders a `role="tablist"` nav with two buttons and switches the
`<main>` body on `tab`. This is what the top-level "Analysis" nav entry mounts.
Quant internals are documented separately (cross-ref above).

```tsx
type SubTab = "analysis" | "quant";
const [tab, setTab] = useState<SubTab>("analysis");
// …
{tab === "analysis" && <AnalysisTab />}
{tab === "quant" && <QuantPage />}
```

### AnalysisTab.tsx — read-only results board (IMPORTANT)

**Responsibility.** Render the latest run's results: columns, chart, diff,
export. Also surfaces STT analysis/eval panels when an STT result exists (out of
scope here).
**Why.** Selection and running are elsewhere; this is the pure *view* of
`store.rows`.
**What/How.** Subscribes to `rows`. Empty state (`rows.length === 0` and no STT)
shows guidance. Otherwise maps `rows → CompareColumn`, then — directly below the
live answers — the full per-token latency metrics (`LatencyTimelines`, the same
panels the **Latency** tab renders; see [frontend-inspector-quant-agentreport](./frontend-inspector-quant-agentreport.md#latencytimelines)),
then `MetricsChart`, `CompareDiff`, `ExportButtons`. `LatencyTimelines` renders
nothing until a run carries a `timeline`, so runs without per-token data are
unaffected.

```tsx
const rows = useCompareStore((s) => s.rows);
// …
<div className="flex gap-2 overflow-x-auto" data-testid="compare-columns">
  {rows.map((r) => <CompareColumn key={r.model} row={r} />)}
</div>
<LatencyTimelines active={topView === "compare"} />  {/* full Latency metrics under the answer */}
<MetricsChart /> <CompareDiff /> <ExportButtons />
```

### CompareColumn.tsx — one model's output card

**Responsibility.** Render a single row: model label (`useModelLabel`), a status
badge (`[Waiting]`/`[Loading model…]`/`[Running]`/`[Done]`/`[Cancelled]`/
`[Error]`), the streamed `output` (a spinner placeholder while `loading` with no
output yet), a one-line metrics summary on done (`TTFT … · … tok/s · … tokens`,
omitting null metrics), and an inline error block.
**Why/How.** Status→label and status→Tailwind-class are lookup `Record`s;
`formatMetrics` filters out nulls so a not-measured TTFT never shows as `0ms`.

### MetricsChart.tsx — ASCII bar chart (IMPORTANT)

**Responsibility.** Two stacked bar groups — THROUGHPUT (tok/s, green) and TTFT
(ms, blue) — across all `done` rows, in a monospace box.
**Why.** Pure-CSS/text bars (no chart lib here) keep it deterministic and
testable; **null metrics render "Not available", never a fake 0**.
**What/How — the math.**
- **Axis limit** snaps the data max up to a round step: throughput `ceil(max/10)*10`
  (min 10), TTFT `ceil(max/500)*500` (min 500). No hardcoded "hardware max".
- **`niceTicks(limit, approxCount, round)`** emits ticks at a step of
  `max(round, ceil(limit/approxCount/round)*round)` — ~`approxCount` evenly
  rounded ticks derived from the data's own scale.
- **Bar fill** = `min(50, round(val/limit * 50))` blocks (`▓`), the bar being
  `totalChars = 50` monospace cells wide.
- **Diff caret** appears only when **≥2 models actually reported** a throughput
  (filters nulls): shows `+Δ tok/s (<leader> leads)` and positions a `▲` at the
  *smaller* of the two values' fill index.

```tsx
const maxThroughput = throughputVals.reduce((max, v) => (v.val != null ? Math.max(max, v.val) : max), 0);
const throughputLimit = maxThroughput > 0 ? Math.ceil(maxThroughput / 10) * 10 : 10;
const niceTicks = (limit, approxCount, round) => {
  const step = Math.max(round, Math.ceil(limit / approxCount / round) * round);
  const ticks = []; for (let t = step; t <= limit + 0.001; t += step) ticks.push(t);
  return ticks;
};
const filledCount = Math.min(totalChars, Math.round((v.val / throughputLimit) * totalChars));
// nulls →  <span className="italic">Not available</span>
```

`getShortName` derives a ≤4-char tag (e.g. `Lla3`) for the row gutter from the
model name. (`format/metricsBars.ts`'s `barRows` is a parallel, pure normalizer —
see format section — but the live chart computes its own fills inline.)

### CompareDiff.tsx + DiffView.tsx — token diff (IMPORTANT)

**Responsibility.** Pairwise word-level diff of two finished outputs.
**Why.** Only meaningful for exactly two outputs; `CompareDiff` returns `null`
unless `rows.filter(status==="done").length === 2`. A toggle button
(`Diff: A → B`) reveals `DiffView`.
**What/How.** `DiffView` calls `diffSegments(a, b)` (from `format/diff.ts`) and
renders each segment as a `<span>`: `ins` = green (added in `b`), `del` = red
strike-through (removed from `a`), `eq` = plain.

```tsx
// CompareDiff
const done = rows.filter((r) => r.status === "done");
if (done.length !== 2) return null;
const [a, b] = done;
{show && <DiffView a={a.output} b={b.output} />}
// DiffView
const CLASS = { ins: "bg-green-100 text-green-800",
                del: "bg-red-100 text-red-800 line-through", eq: "" };
segs.map((s, i) => <span key={i} className={CLASS[s.kind]}>{s.text}</span>)
```

### ExportButtons.tsx — Markdown/JSON export

**Responsibility.** Two buttons (disabled when no rows). On click: open a native
save dialog (`@tauri-apps/plugin-dialog`, default name
`quantamind-compare-<ts>.<fmt>`), `buildReport({ prompt, systemPrompt, strategy,
hardwareSnapshot, selectedModels, rows, installed })`, render via `toMarkdown` or
`toJson`, then `saveCompareReport(path, format, contents)` → `save_compare_report`.
IPC errors surface inline via `formatIpcError`.

### controls/ — Workspace-side controls

| File | Responsibility | Notes |
|---|---|---|
| `controls/HardwareSummary.tsx` | Fetches `get_hardware_snapshot` into the store on mount (with Retry/Refresh nonce); shows `total · available` memory ("Unified memory" on Apple Silicon else "RAM"). Calls `assessStrategies` and renders per-strategy verdict badges **only with ≥2 models**; a single tight model gets one "Needs … · Risky/Won't fit" warning. | Snapshot lives in `compareStore`, shared with the picker + run guard. |
| `controls/MultiRun.tsx` | The "Compare (N)" trigger shown in the Workspace. Enabled when not running, a current workspace prompt is non-empty, and ≥1 model selected. On Run: copies `current.user`/`current.system` into the store, navigates `setTopView("compare")`, calls `useCompareRun().start()`. Toggles to "Cancel all" while running. | Backend per model resolved in `useCompareRun`. |
| `controls/RunStrategyPicker.tsx` | Radio cards for `sequential`/`parallel` writing `store.strategy`; each card shows its `assessStrategies` verdict badge (OK/Risky/Won't fit) and help text (`max(model)` vs `sum(models)` memory). | Pure read of selected models + snapshot. |

---

## `hooks/useCompareRun.ts` — run orchestration (IMPORTANT)

**Responsibility.** The single hook that **starts the event bus, guards the run,
fires `run_compare`, and exposes cancel**. The store is filled *by events*, not
by this hook directly (except `initRun`).
**Why.** Centralizes the "can this run?" gate (model count, prompt, feasibility)
and the args assembly (per-model backends, params, keep-alive) so components
stay dumb.
**What/How.**
- On mount: `void startCompareEventBus()` (idempotent — see event bus).
- `start()` reads fresh state via `getState()` (not React props, so it's
  callback-stable):
  1. **Guards** — ≥1 selected model, non-empty prompt, and the chosen strategy's
     `assessStrategies` verdict isn't `wont_fit` (else a sized error string).
  2. `initRun(selectedModels)` seeds one `pending` row per model + `isRunning`.
  3. `runCompare({...})` — **`backends` is built per model** (`m.backend`; each
     model is coupled to its own weight format); `params`/`perModelParams` come
     from `useParamsStore` (per-model omitted when `sharedParams`); `keepAlive:
     -1` only when `keepLoaded`, else omitted so the backend uses its
     memory-safe strategy default.
  4. On throw: `setStartError(formatIpcError(e))` and `finishRun()`.
- `cancelAll()` → `stopCompare()` (best-effort, swallows errors).

```ts
useEffect(() => { void startCompareEventBus(); }, []);

const start = useCallback(async () => {
  const { prompt, systemPrompt, strategy, hardwareSnapshot, initRun, finishRun } = useCompareStore.getState();
  const selectedModels = useSelectedModelStore.getState().selectedModels;
  if (selectedModels.length === 0) { setStartError("Pick at least one model in the header."); return; }
  if (prompt.trim().length === 0) { setStartError("Type a prompt first."); return; }
  const matrix = assessStrategies(selectedModels, hardwareSnapshot);
  if (matrix && matrix[strategy].status === "wont_fit") { /* …sized error… */ return; }
  initRun(selectedModels);
  const backends = selectedModels.map((m) => m.backend);   // coupled to weight format
  const { globalParams, keepLoaded, sharedParams, perModelParams } = useParamsStore.getState();
  await runCompare({
    models: selectedModels.map((m) => m.name), prompt, strategy,
    ...(system ? { system } : {}), params: globalParams,
    ...(sharedParams ? {} : { perModelParams }), backends,
    ...(keepLoaded ? { keepAlive: -1 } : {}),
  });
}, []);
```

---

## `state/` — store, event bus, row model, strategy

### compareStore.ts — the run model (IMPORTANT)

**Responsibility.** Zustand store holding the prompt/system, hardware snapshot,
chosen strategy, the **`rows` array** (the heart of the tab), and `isRunning`.
All event-driven mutations are immutable `rows.map` patches keyed by `model`.
**Why/How — shape + the event mutators.**

```ts
interface CompareStore {
  prompt: string; systemPrompt: string;
  hardwareSnapshot: HardwareSnapshot | null;
  strategy: StrategyId;                 // "sequential" | "parallel"
  rows: CompareRow[]; isRunning: boolean;
  // setters: setPrompt/setSystemPrompt/setHardwareSnapshot/setStrategy
  initRun(models): void;                // rows = newRow per model, isRunning=true
  setRowLoading(model, modelId): void;  // pending → loading
  appendToken(model, modelId, text): void; // → running, output += text, stamp startedAt
  setRowDone(p): void;                  // → done + metrics{ttft_ms,tokens_per_sec,token_count,timeline,stats}
  setRowCancelled(p): void; setRowError(p): void;
  setSingleRun(row): void;              // bridge a single run_prompt run into rows
  finishRun(): void;                    // isRunning=false; leftover pending → cancelled
  reset(): void;
}
```

Key behaviors:
- `appendToken` lazily flips `pending/loading → running`, stamps `startedAt` on
  first token, and keeps the first `modelId` it saw (`r.modelId ?? modelId`).
- `setRowDone` records measured metrics verbatim (nulls preserved) and stamps
  `endedAt`.
- `finishRun` mops up: any row still `pending` when the run ends becomes
  `cancelled` (so a model that never emitted doesn't hang on "Waiting").
- `setSingleRun` is the bridge so a **single** Workspace `run_prompt` run shows
  up in this tab as one row (re-exports `CompareModel`/`CompareRow`/`RowStatus`).

### compareRow.ts — row type + helpers

`CompareModel = { name; size_bytes }`. `RowStatus = pending | loading | running |
done | cancelled | error`. `CompareRow` carries `model`, `modelId`, `status`,
`output`, nullable `metrics` (`ttft_ms`/`tokens_per_sec`/`token_count` +
optional `timeline`/`stats`), `error`, `startedAt`/`endedAt`. `newRow(model)`
seeds a `pending` blank; `updateRow(rows, model, patch)` is the immutable
keyed-patch helper the store mutators use.

### compareEventBus.ts — event → store bridge (IMPORTANT)

**Responsibility.** Subscribe **once** to the six backend compare events and route
each into the store after Zod validation.
**Why.** Single idempotent subscription (guarded by a module-level `starting`
promise) avoids duplicate listeners on re-mount; **payloads are validated** so
IPC drift is logged (`console.error`) rather than corrupting state.
**What/How.** Each `listen()` `safeParse`s with the matching schema from
`shared/ipc/events/compare_events.ts`; on success calls the store mutator, on
failure logs "IPC payload drift". `compare-run-done` → `finishRun()`.
`__resetCompareEventBusForTests()` clears the singleton.

```ts
let starting: Promise<void> | null = null;
export function startCompareEventBus(): Promise<void> {
  if (starting) return starting;          // idempotent
  starting = (async () => {
    await listen(EVENT_COMPARE_TOKEN, (e) => {
      const p = CompareTokenPayloadSchema.safeParse(e.payload);
      if (p.success) useCompareStore.getState().appendToken(p.data.model, p.data.model_id, p.data.text);
      else console.error("IPC payload drift (compare-token):", p.error.issues, e.payload);
    });
    // …loading / done / cancelled / error …
    await listen(EVENT_COMPARE_RUN_DONE, () => useCompareStore.getState().finishRun());
  })();
  return starting;
}
```

Event → mutator map:

| Event | Schema | Store mutator |
|---|---|---|
| `compare-loading` | `CompareLoadingPayloadSchema` | `setRowLoading(model, model_id)` |
| `compare-token` | `CompareTokenPayloadSchema` | `appendToken(model, model_id, text)` |
| `compare-done` | `CompareDonePayloadSchema` | `setRowDone(payload)` |
| `compare-cancelled` | `CompareCancelledPayloadSchema` | `setRowCancelled(payload)` |
| `compare-error` | `CompareErrorPayloadSchema` | `setRowError(payload)` |
| `compare-run-done` | (none) | `finishRun()` |

### strategy.ts — feasibility math

**Responsibility.** Pure (no React/IPC) memory feasibility per strategy.
**Why/How.** `required_bytes = ceil(size_bytes * 1.3)` (SAFETY multiplier for KV
cache + ctx buffer). `verdict(need, avail)`: `wont_fit` if `need > avail`,
`risky` above 70% of available, else `ok`. `assessStrategies(models, snapshot)`
returns `{ sequential: max(required), parallel: sum(required) }` verdicts, or
`null` when no snapshot / no models.

```ts
const SAFETY = 1.3; const RISKY_FRACTION = 0.7;
const required = models.map((m) => Math.ceil(m.size_bytes * SAFETY));
return {
  sequential: { status: verdict(Math.max(...required), avail), required_bytes: Math.max(...required) },
  parallel:   { status: verdict(sumReq, avail),               required_bytes: sumReq },
};
```

> Note: `StrategyId` here is `sequential | parallel`. The IPC type
> (`shared/ipc/compare/compare.ts`) additionally allows `sequential_skippable`;
> the UI only offers the two assessable strategies.

---

## `format/` — report + presentation builders

### buildReport.ts — store → AnalysisDocument (IMPORTANT)

**Responsibility.** Assemble the exportable `AnalysisDocument` (a populated subset
of `docs/reference.md#analysis-schema`) from current store + installed-models
data. Pure; `now`/`uid` are injectable for tests.
**Why/How.** Builds `models` (id = `model.<slug>`; pulls `display_name`/`family`/
`quantization`/`backend` from installed info, `size_bytes` from selection — each
**omitted when absent**), one `prompts[0]` (system omitted when blank), and one
`runs[i]` per row (status `done → completed`, metrics renamed to
`tokens_per_second`/`total_tokens_generated`, errors arrayed). `environment`
folds in the hardware snapshot (memory + `gpu.unified_memory`). Title is
`Run: X` for ≤1 model else `Compare: A · B · …`. `reproducibility` flags
non-deterministic (seeds unpinned).

```ts
const runs = input.rows.map((r, i) => ({
  id: `run.${i}`, prompt_id: "prompt.main", model_id: modelId(r.model),
  started_at: r.startedAt, completed_at: r.endedAt,
  status: r.status === "done" ? "completed" : r.status,
  metrics: r.metrics
    ? { ttft_ms: r.metrics.ttft_ms, tokens_per_second: r.metrics.tokens_per_sec,
        total_tokens_generated: r.metrics.token_count }
    : null,
  output: { text: r.output, truncated: false },
  warnings: [], errors: r.error ? [{ kind: r.error.kind, message: r.error.message }] : [],
}));
```

### markdownReport.ts — AnalysisDocument → Markdown (IMPORTANT)

**Responsibility.** Render the document as a human-readable `.md` report.
**Why/How.** Header block: run-at, strategy, a hardware line (`formatBytes` total
· available, "Apple Silicon, unified" vs "RAM"), and a models line (each with
size). The prompt is block-quoted line by line. Then one `## <model>` section per
run with started/ended, an error line (short-circuits the rest) or a metrics line
+ optional `Status: cancelled`, then the raw output. Footer = `REPORT_FOOTER`
(`branding.ts`).

```ts
export function toMarkdown(d: AnalysisDocument): string {
  const lines = ["# QuantaMind Compare Report"];
  lines.push(`- Run at: ${d.created_at}`);
  if (d.run_strategy) lines.push(`- Strategy: ${d.run_strategy}`);
  const hw = hardwareLine(d); if (hw) lines.push(hw);
  lines.push(selectedLine(d), "", "## Prompt");
  for (const ln of (d.prompts[0]?.user_prompt ?? "").split("\n")) lines.push(`> ${ln}`);
  for (const r of d.runs) lines.push(...runSection(d, r));   // ## <model> + metrics + output
  lines.push("", "---", "", `_${REPORT_FOOTER}_`);
  return lines.join("\n");
}
```

### jsonReport.ts — AnalysisDocument → JSON

`toJson(d) = JSON.stringify(d, null, 2)`. One line; the schema (`schema.ts`) is
the contract.

### schema.ts — the export document contract

TypeScript interfaces for the exported document: `DocModel`, `DocPrompt`,
`DocRunMetrics` (`ttft_ms` / `tokens_per_second` / `total_tokens_generated`),
`DocRun`, `DocEnvironment`, and the top-level `AnalysisDocument`
(`schema_version`, `document_id`, `document_type: "bench-report" | "analysis"`,
`title`, `created_at`, optional `run_strategy`/`environment`, `models`,
`prompts`, `runs`, `findings`, `verdicts`, `reproducibility`). A populated subset
of `docs/reference.md#analysis-schema`; everything past the required spine is
optional.

### metricsBars.ts — pure bar normalizer

`barRows(rows, metric)` → `{ model, value, fraction }[]` for `done` rows that
have a numeric metric; `fraction = value/max` (0 when max is 0). A pure,
unit-tested helper paralleling the live `MetricsChart` math.

### Trivial format files

| File | Purpose |
|---|---|
| `branding.ts` | `BRAND_NAME`, `BRAND_URL`, `REPORT_FOOTER` constants for the report footer. |
| `ulid.ts` | `ulid(time, rnd)` — 26-char Crockford-base32 ULID (10 time + 16 random), lexicographically sortable; `document_id` source for `buildReport`. |

---

## Files at a glance

| Area | File | One-liner |
|---|---|---|
| components | `AnalysisPage.tsx` | Analysis/Quant sub-tab host (local state). |
| components | `AnalysisTab.tsx` | Read-only results board: columns + chart + diff + export. |
| components | `CompareColumn.tsx` | One model's streamed output card + status + metrics. |
| components | `MetricsChart.tsx` | ASCII tok/s + TTFT bars; data-derived ticks; null→"Not available". |
| components | `CompareDiff.tsx` | Toggle + gate (exactly 2 done) for the pairwise diff. |
| components | `DiffView.tsx` | Renders `diffSegments` as colored spans. |
| components | `ExportButtons.tsx` | Save dialog → buildReport → toMarkdown/toJson → save_compare_report. |
| controls | `HardwareSummary.tsx` | Fetch snapshot; per-strategy verdict badges. |
| controls | `MultiRun.tsx` | "Compare (N)" trigger; copies prompt, navigates, starts run. |
| controls | `RunStrategyPicker.tsx` | sequential/parallel radio cards with verdicts. |
| hooks | `useCompareRun.ts` | Start bus, guard, assemble args, `run_compare`/`stop_compare`. |
| state | `compareStore.ts` | Zustand: prompt/snapshot/strategy/rows/isRunning + event mutators. |
| state | `compareRow.ts` | `CompareRow`/`RowStatus` types + `newRow`/`updateRow`. |
| state | `compareEventBus.ts` | Idempotent listen → Zod-validate → store mutator. |
| state | `strategy.ts` | Pure feasibility math (1.3× safety, 70% risky). |
| format | `buildReport.ts` | Store → `AnalysisDocument`. |
| format | `markdownReport.ts` | Document → Markdown. |
| format | `jsonReport.ts` | Document → pretty JSON. |
| format | `schema.ts` | `AnalysisDocument` interface contract. |
| format | `metricsBars.ts` | Pure `{value, fraction}` bar normalizer. |
| format | `diff.ts` | `diffSegments` via diff-match-patch (semantic cleanup). |
| format | `branding.ts` | Report footer constants. |
| format | `ulid.ts` | ULID document-id generator. |

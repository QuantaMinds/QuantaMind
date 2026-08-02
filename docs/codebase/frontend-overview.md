# Frontend Overview — App Shell & Shared Layer

> The React 19 + TypeScript shell of the QuantaMind Tauri 2 desktop app, plus
> the cross-cutting **shared layer**: the typed IPC client to Rust, the global
> Zustand stores, the shared UI kit, and the pure model/format helpers.
>
> Source roots: `frontend/src/*.tsx`, `frontend/src/*.ts`, `frontend/src/shared/`.
> Per-feature UI lives under `frontend/src/features/` — see the feature pages
> cross-linked below.

---

## Overview

### Why a shared layer + a central IPC

**Separation of concerns (CLAUDE.md rule 3) + the layering law (architecture.md):**
features must not import each other. Anything two features both need lives in
`shared/`, and the shell (`App.tsx`, `GlobalControls`, `WorkspaceSidebar`)
*composes* features rather than letting them reach across. The two things every
feature needs are (a) a typed way to call Rust and (b) a few pieces of global
state (selected backend, selected model, inference params). Those are the shared
layer.

**The IPC layer** is a thin, typed wrapper over Tauri's `invoke()`. Each Rust
`#[tauri::command]` gets one TypeScript function under
`shared/ipc/<domain>/<file>.ts` that calls `invoke("command_name", args)`, casts
or Zod-validates the result, and re-exports the result type. Features import
those wrappers; they never call `invoke()` with a bare string. This gives one
place to (1) name a command, (2) shape its args/result, (3) validate untrusted
payloads at the boundary (MEMORY: *validate JSON via strict struct* — mirrored on
the TS side with Zod schemas), and (4) normalize errors into user copy.

### What the app shell is

`main.tsx` mounts `<App/>` in React StrictMode. `App.tsx` is the composition
root. It renders, top to bottom:

- **`AppHeader`** — back button (drives `navStore.goBack`), logo/title, the
  global `GlobalControls`, and (on the Workspace view only) a History toggle.
- **`GlobalControls`** — the header control group: `ServerControl`,
  `BackendSelector`, `ModelSelector`, `ParamsControl`. These read/write the
  global stores, so every page sees the same selection.
- **`OnboardingCoach`** — first-run guidance (feature).
- **The top-nav `<nav>`** — ten tabs from the `TABS` array, each a button that
  calls `setView(t.id)`.
- **Ten view containers** — each is `<div hidden={view !== id}>` wrapping the
  feature page. All ten pages stay mounted; only the active one is shown
  (`hidden` toggled), so per-page state survives tab switches.
- **Always-mounted overlays:** `FeedbackButton`, `HistoryPanel`,
  `CheatsheetModal`, `StartupUpdate`, `ToastHost`.

Two app-wide effects fire once on mount: `startInstalledModelsBus()` (begins
streaming the installed-model list into its store) and
`modelSettingsStore.load()`. Two hooks wire global behavior: `useAutoSave()`
(workspace) and `useGlobalHotkeys()` (see hotkeys below).

### How routing works

There is no router library. Routing is a single Zustand value:
`navStore.topView` (a `TopView` union of the ten tab ids). `setTopView(v)`
switches the view **and** pushes the previous view onto a bounded `history` stack
(last 20); `goBack()` pops it. `App` subscribes to `topView` and shows the
matching container. This lets any deep component navigate without prop-drilling —
e.g. the Workspace "Add model" button calls `navStore.setTopView("models")`, and
the back button returns the user to where they were.

### The ten nav tabs

`App.tsx` `TABS` array maps each `TopView` id → page component → feature doc
(identical to [`README.md`](README.md)):

| Tab (`TopView` id) | Label | Page component | Doc |
|---|---|---|---|
| `workspace` | Workspace | `features/workspace/components/Workspace.tsx` | [frontend-workspace](frontend-workspace.md) |
| `compare` | Analysis | `features/compare/components/AnalysisPage.tsx` | [frontend-compare-analysis](frontend-compare-analysis.md) |
| `inspector` | Latency | `features/inspector/components/InspectorPage.tsx` | [frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md) |
| `eval` | Tests | `features/eval/components/EvalPage.tsx` | [frontend-eval](frontend-eval.md) |
| `audit` | Audit | `features/audit/components/AuditPage.tsx` | [frontend-support-features](frontend-support-features.md) |
| `agentReport` | Agent Report | `features/agentReport/components/AgentReportPage.tsx` | [frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md) |
| `models` | Models | `features/models/components/ModelsPage.tsx` | [frontend-models](frontend-models.md) |
| `downloads` | Downloads | `features/models/components/DownloadsPage.tsx` | [frontend-models](frontend-models.md) |
| `settings` | Settings | `features/settings/components/SettingsPage.tsx` | [frontend-support-features](frontend-support-features.md) |
| `docs` | Docs | `features/docs/components/DocsPage.tsx` | [frontend-support-features](frontend-support-features.md) |

The Quant view is a **sub-tab merged into Analysis**; it shares the readiness
data model with Latency/Agent Report and is documented on
[frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md).

---

## 1. App shell files

The shell components are mostly presentational composition; the load-bearing
logic is the routing in `navStore` (§3) and the model picker. Compact summary:

| File | Purpose |
|---|---|
| `main.tsx` | React root; renders `<App/>` in `StrictMode`; imports `index.css`. |
| `App.tsx` | Composition root: `TABS`, top-nav, ten `hidden`-toggled view containers, always-mounted overlays, mount-time effects + global hooks. |
| `AppHeader.tsx` | Back button → `navStore.goBack`; logo/title; mounts `GlobalControls`; History toggle on Workspace view. |
| `GlobalControls.tsx` | Composes the LLM control group at shell level (features don't import each other). |
| `BackendSelector.tsx` | Global LLM-backend dropdown (llama.cpp / +vLLM on Apple Silicon). Writes `backendStore.selectedBackend`; health dot from `isHealthy`. `useVLlmBackend`/`useLlamaBackend` poll health. |
| `ModelSelector.tsx` | Global model picker (see below). |
| `ParamsControl.tsx` | Inference-params popover writing `paramsStore`; "same for all" toggle → per-model overrides for llama.cpp 2+ compares; "Use max" pulls `num_ctx` from `useVramFit`. Reuses `ParamRow`/`PARAMS`. |
| `WorkspaceSidebar.tsx` | Workspace left rail (`WorkspaceSwitcher` + `FilesSection`); visibility from `uiStore.sidebarVisible`. |
| `appHotkeys.ts` | `useGlobalHotkeys` — wires global shortcuts to store actions (see §5). |
| `vite-env.d.ts` | Vite client type reference only. |

**`ModelSelector.tsx`** carries the one piece of real shell logic worth
showing — the single/multi-select rule and the cross-store wiring. llama.cpp is
multi-select (1 → single run, 2+ → a Workspace compare); llama.cpp are
single. It filters the installed list to the selected backend, drops embedding
models (`isEmbeddingModel`), de-dupes by digest, and writes the **global**
`selectedModelStore`:

```ts
const multi = selectedBackend === "llama_cpp";
const generative = dedupeByDigest(
  list.filter((m) => !isEmbeddingModel(m) && m.backend === selectedBackend),
);
const pick = (m) => {
  const entry = { name: m.name, backend: m.backend, size_bytes: m.size_bytes, path: m.path };
  if (!multi) { setSelectedModels(has(m.name) ? [] : [entry]); setOpen(false); return; }
  setSelectedModels(has(m.name) ? selectedModels.filter((s) => s.name !== m.name)
                                : [...selectedModels, entry]);
};
```

---

## 2. The typed IPC layer (`shared/ipc/`)

The frontend↔Rust contract. Layout: `core/` (the primitives every wrapper
relies on), `events/` (schemas for Rust→UI streamed events), `cache.ts`, and one
folder per backend subsystem holding the thin per-command wrappers.

> **Note on `core/client.ts`:** despite the name there is **no** single
> `invoke()` wrapper function — each domain wrapper calls Tauri's `invoke`
> directly and applies the shared primitives (`withTimeout`, the Zod schemas,
> `formatIpcError`) as needed. `client.ts` itself is just the health-probe
> command group. The "central IPC" is a *convention* enforced across
> `shared/ipc/`, not one chokepoint function.

### 2.1 `core/` — the primitives

**File:** `shared/ipc/core/timeout.ts`
**Responsibility:** Bound any `invoke()` promise so a hung backend can't lock the
UI in "running" forever.
**Why:** A large model can take a long time to load, but a *dead* server hangs
indefinitely; the UI must surface a `TimeoutError` instead of spinning.
**What:** `TimeoutError extends Error` + `withTimeout(promise, ms, label)`.
**How/Where used:** Wrappers that can stall (model load, install) race their
`invoke` against this; the catch path feeds `formatIpcError`/`classifyError`.

```ts
export function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new TimeoutError(label, ms)), ms);
    promise.then((v) => { clearTimeout(timer); resolve(v); },
                 (e) => { clearTimeout(timer); reject(e); });
  });
}
```

**File:** `shared/ipc/core/error.ts`
**Responsibility:** Turn any thrown value (Tauri string, `Error`, `ZodError`,
`{message}`) into one readable message; map known network failures to friendly
copy.
**Why:** Tauri rejects with raw strings, Rust errors arrive as serialized
objects, and Zod's `.message` is a raw JSON issue array — the UI needs one
normalizer. (Pairs with MEMORY *no fake metrics* / *guardrail + popup on bugfix*:
errors are surfaced honestly, not swallowed.)
**What:** `rawMessage(e)` (ordered: ZodError → `Error` → string → `{message}` →
JSON → `String`), `friendly(msg)` (Connection-refused / os error 61 →
"the local server is not running…"), and the public `formatIpcError(e)`.
**How/Where used:** Every catch block across features routes through
`formatIpcError`; richer cards use `classifyError` (below).

```ts
export function formatIpcError(e: unknown): string {
  return friendly(rawMessage(e));
}
```

**File:** `shared/ipc/core/errorInfo.ts`
**Responsibility:** Upgrade a raw error to a structured, actionable
`ErrorInfo { title, body, learnMore?, actionHint? }`.
**Why:** Toasts want a string; the `ErrorCard` wants a title + body + a primary
action (Retry / Start llama.cpp / Open Models / Open Settings) and a docs link.
**What:** `classifyError(e)` — most-specific-first branches (llama.cpp down →
model-not-found → OOM → timeout → generic). `learnMore` anchors match
`docs/reference.md#troubleshooting`.
**How/Where used:** Rendered by `shared/ui/ErrorCard` across feature error
states.

**File:** `shared/ipc/core/types.ts`
**Responsibility:** Shared IPC types. `AppError {kind, message}` (kind union
mirrors Rust's `errors.rs`), `HealthStatus {available, version}`. Type-only.

**File:** `shared/ipc/core/client.ts`
**Responsibility:** The health-probe command group. `listModels`,
`checkllama.cppHealth`/`checkVLlmHealth`/`checkLlamaHealth`, and `healthFor(backend)`
dispatching to the right one (used by the batch pre-flight to fail fast on a down
server). Backend: see [backend-overview](backend-overview.md) /
[backend-inference-backends](backend-inference-backends.md).

### 2.2 `events/` — Rust→UI streamed-event schemas

These files **define Zod schemas + event-name constants**, not subscribers. The
actual `listen()` subscription lives in the consuming feature (e.g.
`features/workspace/hooks/useStreamingRun.ts`, `features/compare/state/compareEventBus.ts`),
which imports these schemas and `safeParse`s each payload at the boundary — Rust
streams untrusted JSON, so the schema is the validation gate (mirrors MEMORY
*validate JSON via strict struct*).

| File | Event constants | Schemas (key payloads) |
|---|---|---|
| `events/events.ts` | `prompt-token`, `prompt-done`, `prompt-cancelled` | `TokenPayloadSchema`, `TokenTimingSchema`, `GenerateStatsSchema` (all fields optional/nullable → "not measured", never 0), `DonePayloadSchema` (ttft, tok/s, timeline, stats), `CancelledPayloadSchema`. |
| `events/compare_events.ts` | `compare-token/done/cancelled/error/run-done/loading` | Per-model variants keyed by `model_id` + `model`; reuses `GenerateStatsSchema`/`TokenTimingSchema`. |
| `events/pull_events.ts` | `pull-progress` | `PullProgressSchema` — a **discriminated union** on `phase` (`pulling_manifest`/`downloading`/`verifying`/`writing`/`success`/`failed`), wrapped in `PullProgressEventSchema {pull_id, name, progress}`. |

Representative subscriber pattern (in a feature hook, not in `shared/`):

```ts
import { listen } from "@tauri-apps/api/event";
import { EVENT_TOKEN, TokenPayloadSchema } from "../../../shared/ipc/events/events";
const un = await listen(EVENT_TOKEN, (e) => {
  const r = TokenPayloadSchema.safeParse(e.payload);
  if (r.success) append(r.data.text);   // drop malformed payloads at the boundary
});
```

The `DonePayloadSchema` Zod contract is unit-tested in
`shared/ipc/__tests__/events.test.ts` (timeline required, entries `t_ms ≥ 0`,
`n ≥ 1`).

### 2.3 `cache.ts`

`clearAppCache()` → `invoke("clear_app_cache")`, returns bytes freed. Clears
regenerable caches (eval history, batch reports, traces, cliff measurements,
recent-workspace list); never touches downloaded models, custom collections,
readiness profiles, or user settings. Used by Settings.

### 2.4 Per-domain command wrappers (inventory)

Each file wraps one backend command group: `invoke("command", args)`, cast or
Zod-validate, re-export the type. Grouped by subsystem with the backend doc each
maps to.

| Wrapper file | Backend commands wrapped | Backend doc |
|---|---|---|
| `compare/compare.ts` | `save_compare_report` | [backend-overview](backend-overview.md) |
| `compare/hardware.ts` | `get_hardware_snapshot` | [backend-overview](backend-overview.md) |
| `eval/batch.ts` | `run_batch_eval`, `stop_batch_eval` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/cliff.ts` | `run_context_cliff`, `stop_context_cliff`, `save_cliff_result`, `get_cliff_results` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/evals.ts` | `list_evals`, `run_eval_task` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/matrix.ts` | `run_collection_matrix`, `load_collection_history` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/queue.ts` | `check_unfinished_run`, `resume_batch_eval`, `discard_run` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/readiness.ts` | `list_readiness_profiles`, `save_/delete_readiness_profile`, `assess_readiness` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/registry.ts` | `get_builtin_tasks`, `list_/get_builtin_collection`, `list_/load_/save_/delete_/import_custom_collection`, `read_text_capped` | [backend-eval-engine](backend-eval-engine.md) |
| `eval/toolcall.ts` | `run_toolcall_eval`, `trace_toolcall_task`, `load_toolcall_trace` | [backend-eval-engine](backend-eval-engine.md) |
| `models/gguf.ts` | `inspect_gguf`, `install_local_gguf` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `models/hf_browse.ts` | `hf_search`, `hf_repo_files`, `hf_repo_all_files`, `hf_model_card` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `models/hf_install.ts` | `install_hf_gguf`, `cancel_hf_install` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `models/llama_start.ts` | `start_/stop_llama_server`, `list_llama_models`, `delete_llama_model` | [backend-inference-backends](backend-inference-backends.md) |
| `models/local_install.ts` | (Zod schema only — no command) | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `models/vllm.ts` | `list_vllm_models`, `install_vllm_model`, `delete_vllm_model` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `models/vllm_start.ts` | `start_/stop_vllm_server`, `vllm_server_status` | [backend-inference-backends](backend-inference-backends.md) |
| `models/model_settings.ts` | `get_model_settings`, `set_model_temperature` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `models/llama_cpp_start.ts` | `start_llama_cpp`, `stop_llama_cpp` | [backend-inference-backends](backend-inference-backends.md) |
| `models/storage.ts` | `get_installed_models_with_stats`, `remove_model`, `get_disk_usage`; exports `BackendKind` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `prompts/templates.ts` | `list_prompt_templates` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `publish/export.ts` | `save_readiness_image` | [backend-publish](backend-publish.md) |
| `publish/preview.ts` | `preview_publish_payload` | [backend-publish](backend-publish.md) |
| `publish/publish.ts` | `publish_to_board`, `start_login` | [backend-publish](backend-publish.md) |
| `settings/settings.ts` | `get_storage_path`, `validate_storage_path` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `settings/userSettings.ts` | `get_user_settings`, `set_user_settings`, `resolve_models_folder` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `system/feedback.ts` | (pure — `buildFeedbackMailto`, no command) | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `system/inspect.ts` | `inspect_model`, `estimate_kv_cache_bytes` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `system/onboarding.ts` | `scaffold_onboarding_workspace`, `pull_model` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `system/process_memory.ts` | `get_llama_cpp_rss` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `system/updater.ts` | Tauri updater plugin: `check`, `relaunch`, `getVersion` (not `invoke`) | [backend-overview](backend-overview.md) |
| `system/vram.ts` | `get_loaded_models` | [backend-models-hf-gguf](backend-models-hf-gguf.md) |
| `workspace/history.ts` | `history_append`, `history_list`, `history_get`, `history_clear`, `history_remove_by_path` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `workspace/prompts.ts` | `load_prompt`, `save_prompt`, `create_prompt`, `rename_path`; exports `InferenceParams`/`InferenceParamsSchema` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |
| `workspace/workspaces.ts` | `open_workspace`, `list_workspace_tree`, `recent_workspaces`, `delete_path`, `close_workspace`, `current_workspace` | [backend-prompt-workspace-system](backend-prompt-workspace-system.md) |

> `workspace/prompts.ts` is the canonical home of `InferenceParams` /
> `InferenceParamsSchema` — `paramsStore` and `ParamsControl` import the type
> from here so the param surface stays in one place.

---

## 3. Shared global state (`shared/state/`)

Five Zustand stores hold the **app-wide** selection (architecture.md rule 7 — one
backend, one model selection, one param set drive every page). Pattern: a typed
interface, then `create<T>((set, get) => ({...}))`. Panels with their own data
(History) keep their own feature store; these five are cross-cutting.

**File:** `shared/state/navStore.ts` — top-nav routing + back history.
**Responsibility:** Single source of truth for which tab is shown, plus a bounded
back stack.
**Why:** No router; any component must be able to navigate (and go back) without
prop-drilling.
**What/How:** `topView: TopView`, `history: TopView[]`, `setTopView`, `goBack`.
`setTopView` pushes the prior view (capped at 20); `goBack` pops it. No-ops if
the target equals the current view or the stack is empty.

```ts
export const useNavStore = create<NavStore>((set) => ({
  topView: "workspace",
  history: [],
  setTopView: (v) => set((s) =>
    v === s.topView ? s : { topView: v, history: [...s.history, s.topView].slice(-20) }),
  goBack: () => set((s) => s.history.length === 0 ? s
    : { topView: s.history[s.history.length - 1], history: s.history.slice(0, -1) }),
}));
```

**File:** `shared/state/backendStore.ts` — selected LLM backend + per-backend
health.
**Why:** The selected backend scopes the whole app (model list, server controls).
A model is bound to its backend's weight format, so switching backends must drop
models that can't run on the new one (MEMORY *backend↔format coupling*).
**What/How:** `selectedBackend`, three `*Healthy: boolean | null` (null = not yet
probed), setters, `isHealthy(b)`. `setSelectedBackend` imperatively reconciles
`selectedModelStore` — no cross-store subscription, no render loop:

```ts
setSelectedBackend: (selectedBackend) => {
  set({ selectedBackend });
  const { selectedModels, setSelectedModels } = useSelectedModelStore.getState();
  const kept = selectedModels.filter((m) => m.backend === selectedBackend);
  if (kept.length !== selectedModels.length) setSelectedModels(kept);
},
```

**File:** `shared/state/selectedModelStore.ts` — the model(s) the app is working
on.
`SelectedModel {name, backend, size_bytes, path?}` carries backend+path so
consumers never re-resolve from the installed list. Multi-select for llama.cpp only.
`selectedModels: SelectedModel[]` + `setSelectedModels`.

**File:** `shared/state/paramsStore.ts` — global inference params (one source of
truth for every run).
`globalParams: InferenceParams` (unset key = omitted → backend default),
`keepLoaded` (llama.cpp `keep_alive` 0 vs -1), `sharedParams` + `perModelParams`
(per-model overrides for llama.cpp 2+ compares). Setters delete-on-undefined so an
unset key is truly absent. Ranges/validation live at the Rust boundary, not here.

```ts
setParam: (key, v) => set((s) => {
  const next = { ...s.globalParams };
  if (v === undefined) delete next[key]; else next[key] = v;
  return { globalParams: next };
}),
```

**File:** `shared/state/uiStore.ts` — lightweight cross-cutting panel toggles.
`sidebarVisible`, `cheatsheetOpen`, `creatingPrompt` + toggles/setters. Driven by
hotkeys and header buttons.

---

## 4. Shared UI kit (`shared/ui/`)

Reusable, mostly-presentational components plus the hotkey/popover primitives.
The two that carry real logic — the hotkey matcher and the popover-dismiss
hook — are shown; the rest are compact.

**File:** `shared/ui/useHotkey.ts` + `shortcuts.ts` — the hotkey system.
`shortcuts.ts` is the **single registry** (`SHORTCUTS`): each entry is
`{id, combo, label, scope}`. `comboFor(id)` resolves a combo; `displayKeys`
renders it (`⌘↵` on mac, `Ctrl+↵` elsewhere). The cheatsheet renders the list;
`appHotkeys.ts` wires each combo to its store action. `useHotkey(combo, handler,
enabled)` registers one global `keydown` listener (handler held in a ref so
callers needn't memoize; `enabled` gates by scope, e.g. workspace-only):

```ts
export function matchCombo(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  const mod = e.metaKey || e.ctrlKey;          // "mod" = Cmd OR Ctrl (cross-platform)
  if (parts.includes("mod") !== mod) return false;
  if (parts.includes("shift") !== e.shiftKey) return false;
  return e.key.toLowerCase() === key;
}
```

`appHotkeys.ts` (`useGlobalHotkeys`, called once in `App`) binds: `new`/`history`/
`files` (workspace-scoped), `open`/`cheatsheet` (global).

**File:** `shared/ui/usePopoverDismiss.ts` — close on outside-click or Escape.
One listener pair attached only while `open`, always cleaned up; `onClose` read
through a ref so the effect re-subscribes only when `open` flips. Used by every
header popover (`ModelSelector`, `ParamsControl`).

| File | Purpose |
|---|---|
| `ErrorCard.tsx` | Actionable error surface: title, body, optional primary action button, "Learn more" link (opens docs via `plugin-shell.open`). Fed by `classifyError`. |
| `CheatsheetModal.tsx` | Modal listing `SHORTCUTS` grouped by scope; Escape/backdrop closes. Driven by `uiStore.cheatsheetOpen`. |
| `Toast.tsx` | `ToastHost` + `useToast()` — transient bottom-center message backed by a tiny private Zustand store with TTL auto-clear. |
| `Tooltip.tsx` | Clip-safe hover tooltip portalled to `<body>` (fixed-position) so an `overflow:hidden` ancestor can't clip it. |
| `InfoButton.tsx` | ⓘ hover popup explaining a tool/metric; dark overlay reads on both light and dark panels. |
| `PlayStopButton.tsx` | Single ▶/⏹ toggle with a busy spinner; testid follows the action (`*-start`/`*-stop`). |
| `Spinner.tsx` | Size/color-overridable spinning SVG indicator. |

---

## 5. Shared models / format / helpers

Pure, dependency-light helpers shared by the model-related features. (Backend
detail: [backend-models-hf-gguf](backend-models-hf-gguf.md).)

**File:** `shared/models/classify.ts` — the model classification logic.
**Why:** Embedding-only models (no `/v1/chat/completions`) would 400 on a generate
request, and the UI has nowhere to route their output — so they're hidden from
the Workspace picker and the Compare multi-select.
**What/How:** `isEmbeddingModel(m)` — family allow-list (`bert`, `nomic-bert`)
plus name heuristics (`embed`, `bge-…`, `all-minilm`). Pure; used by
`ModelSelector` and the compare picker.

```ts
export function isEmbeddingModel(m: ModelLike): boolean {
  const family = (m.family ?? "").toLowerCase();
  if (EMBEDDING_FAMILIES.has(family)) return true;
  const name = m.name.toLowerCase();
  if (name.includes("embed")) return true;
  if (/^bge[-:](m3|small|base|large)/.test(name)) return true;
  if (name.startsWith("all-minilm")) return true;
  return false;
}
```

| File | Purpose |
|---|---|
| `shared/models/modelLabel.ts` | `modelLabel(m)` → `display_name ?? name`; selection/wire calls still use `name`. |
| `shared/models/dedupeDigest.ts` | `dedupeByDigest()` collapses llama.cpp tags sharing one content digest (first wins); digest-less entries (GGUF/vLLM) always kept. Pure. |
| `shared/models/backendSupport.ts` | `servesModelsByName(b)` (llama.cpp only) + two note strings explaining that llama.cpp serve one model at a time (so multi-model/per-quant evals need llama.cpp). |
| `shared/format/bytes.ts` | `formatBytes(n)` ("1.3GB") + `formatDuration(secs)` ("3m 24s") — one canonical formatter so HF/Storage/Compare agree byte-for-byte. |
| `shared/install_error.ts` | `friendlyInstallError(raw)` — maps install-time IPC errors (HF gated repo, rate-limit, bad GGUF, silent llama.cpp rollback, …) to user-facing next steps. Used by the install hooks → `AddModelModal`. |
| `shared/markdown.tsx` | `Markdown`/`parseInline` — tiny markdown subset (bold, inline code, links, h1–h3, bullets) for release notes; links open in the system browser. |

---

## Cross-references

**Feature pages** (under `frontend/src/features/`, composed by `App.tsx`):
[frontend-workspace](frontend-workspace.md) ·
[frontend-compare-analysis](frontend-compare-analysis.md) ·
[frontend-eval](frontend-eval.md) ·
[frontend-models](frontend-models.md) ·
[frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md) ·
[frontend-support-features](frontend-support-features.md).

**Backend pages** (the Rust commands the IPC wrappers call):
[backend-overview](backend-overview.md) (IPC contract, command registration,
`errors.rs`, updater) ·
[backend-inference-backends](backend-inference-backends.md) (server start/stop,
health) ·
[backend-models-hf-gguf](backend-models-hf-gguf.md) ·
[backend-eval-engine](backend-eval-engine.md) ·
[backend-prompt-workspace-system](backend-prompt-workspace-system.md) ·
[backend-persistence](backend-persistence.md) ·
[backend-publish](backend-publish.md).

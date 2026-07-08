# Frontend: Support Features (Settings · Onboarding · Help/Updater · Docs · Feedback · History · Audit)

The "support surfaces" that surround the core local-LLM workflow: first-run
onboarding, app settings + hardware, in-app help with an app updater, a
feedback channel, replayable run history, and the audit/compliance page. None
of these generate tokens; they make the app usable, updatable, and accountable.

Related docs:
- App shell, shared state, IPC core, and `GlobalControls`/`ParamsControl`/`ModelSelector`/`BackendSelector` — [`frontend-overview.md`](frontend-overview.md).
- The Rust commands these surfaces call (settings, history, onboarding scaffold) — [`backend-prompt-workspace-system.md`](backend-prompt-workspace-system.md).
- The Workspace whose runs feed history — [`frontend-workspace.md`](frontend-workspace.md).

---

## Overview

**Why these exist.** Generation is one thin slice. Around it the app needs to:
land a brand-new user on a working model (onboarding); expose hardware + storage
settings; explain every page/metric and ship updates (help + updater); collect
honest feedback; remember and replay every run (history); and surface a
compliance/audit view of saved benchmark runs (audit). Each is its own feature
folder under `frontend/src/features/`, each file owns one responsibility
([`docs/architecture.md`](../architecture.md)).

**How they mount in `App.tsx`.** Three of them are **tabs** (lazy-hidden
`<div hidden=…>` panes switched by the top nav, view state in `navStore`);
four are **always-mounted overlays** that render `null` until they have
something to show. The History panel is a fourth overlay, but it is *toggled*
from a "History" button in `AppHeader` (`useHistoryStore.toggle`), not by the
tab nav.

```tsx
// App.tsx (trimmed)
<AppHeader />            {/* hosts the "History" toggle button */}
<OnboardingCoach />     {/* overlay: null unless first_run_complete === false */}
<nav>…tabs…</nav>
<div hidden={view !== "audit"}><AuditPage /></div>
<div hidden={view !== "settings"}><SettingsPage /></div>
<div hidden={view !== "help"}><HelpPage /></div>
<div hidden={view !== "docs"}><DocsPage /></div>   {/* tab: authored user guides + ⌘K search */}
<FeedbackButton />      {/* overlay: fixed bottom-right pill */}
<HistoryPanel />        {/* overlay: null unless useHistoryStore.open */}
<StartupUpdate />       {/* overlay: null unless a 24h-gated update is found */}
```

### Feature → mount → files → IPC/plugin → backend doc

| Feature | Mount | Key files | IPC / plugin | Backend doc |
|---|---|---|---|---|
| **Settings** | Tab (`settings`) | `settings/components/SettingsPage`, `HardwareSection`, `RemoteBackendsSection` | `get/set_user_settings`, `resolve_models_folder`, `getHardwareSnapshot` | [prompt-workspace-system](backend-prompt-workspace-system.md) (settings) |
| **Onboarding** | Overlay (gated) | `onboarding/components/OnboardingCoach`, `state/onboardingStore`, `steps` | `get/set_user_settings` (`first_run_complete`), `scaffold_onboarding_workspace`, `pull_model` | [prompt-workspace-system](backend-prompt-workspace-system.md) (onboarding) |
| **Help + Updater** | Tab (`help`) + startup overlay | `help/components/HelpPage`, `HelpContent`, `HelpSidebar`, `helpSections`, `UpdateChecker`, `StartupUpdate`; `hooks/useUpdater`; `updateSchedule` | `@tauri-apps/plugin-updater` (`check`), `plugin-process` (`relaunch`), `get/set_user_settings` (`last_update_check_at`) | — (updater is a Tauri plugin) |
| **Docs** | Tab (`docs`) | `docs/components/DocsPage`, `DocsSidebar`, `DocsContent`, `DocsSearch`; `docs/content` (authored guides), `docs/render` (inert markdown + TOC), `docs/search` (no-dep ranked search) | — (all content is bundled in the JS; no backend/IPC) | — (frontend-only) |
| **Feedback** | Overlay (button + modal) | `feedback/components/FeedbackButton`, `FeedbackModal`; `hooks/useSubmitFeedback`; `shared/ipc/system/feedback` | `@tauri-apps/plugin-shell` (`open` mailto) | — (mailto, no backend cmd) |
| **History** | Overlay (header-toggled drawer) | `history/components/HistoryPanel`, `HistoryRow`; `recordRun`; `state/historyStore` | `history_append/list/get/clear/remove_by_path` | [prompt-workspace-system](backend-prompt-workspace-system.md) (history) |
| **Audit** | Tab (`audit`) | `audit/components/AuditPage` | `loadCollectionHistory` (eval matrix), batch CSV/JSON export | — (eval matrix, see eval docs) |

---

## Settings (tab)

`SettingsPage` is a thin host — today it renders only the Hardware section and
is the documented home for future app-level settings (theme, storage path).

| File | What it is |
|---|---|
| `settings/components/SettingsPage.tsx` | Trivial wrapper: `<div><HardwareSection /></div>`. One responsibility = a settings container. |
| `settings/components/HardwareSection.tsx` | Read-only hardware table (CPU, cores, memory, GPU, OS, arch). Fetches `getHardwareSnapshot()` once on mount; renders `formatBytes()` values; unknowns render as `—`/`Not available` (never fabricated). GPU label distinguishes unified (Apple) memory from discrete VRAM total/free. |

**Note.** The *storage path* / models-folder setting is read through
`resolveModelsFolder()` and persisted via `models_folder` on `UserSettings`;
the per-model temperature settings live in `paramsStore`/`GlobalControls` (see
[`frontend-overview.md`](frontend-overview.md)). `SettingsPage` itself does not
yet render those controls — it is the placeholder host the CLAUDE doc-comment
describes.

`UserSettings` is the shared contract (validated by zod, read/written by every
support feature that persists state):

```ts
// shared/ipc/settings/userSettings.ts
export const UserSettingsSchema = z.object({
  theme: z.string().nullable().optional(),
  first_run_complete: z.boolean().default(false),     // ← onboarding gate
  last_update_check_at: z.string().nullable().optional(), // ← updater 24h stamp
  models_folder: z.string().nullable().optional(),    // ← storage path
  stt_engine_dir: z.string().nullable().optional(),
});
```

---

## Onboarding (gated overlay)

The guided first-run flow. A single card mounted above the tab nav that renders
**nothing** once `first_run_complete` is true, and otherwise walks the user
through a 3-step setup derived live from app state.

### `state/onboardingStore.ts` — the first-run gate
- **Responsibility:** owns `complete` (`null` until loaded), `load`, `finish`.
- **Why:** keeps the gate in one place and **fails open** — if settings can't
  load, `complete` becomes `true` so a backend hiccup never traps the user
  behind the coach.
- **How:** `load` reads `get_user_settings().first_run_complete`; `finish`
  optimistically sets `complete=true` then persists `first_run_complete: true`.

```ts
finish: async () => {
  set({ complete: true });                 // optimistic — UI clears immediately
  try {
    const s = await getUserSettings();
    await setUserSettings({ ...s, first_run_complete: true });
  } catch (e) { console.error("onboarding finish persist failed:", e); }
},
```

### `steps.ts` — which step to show
- **Responsibility:** pure derivation of the current step from live state. No
  stored cursor — the step is recomputed from Ollama health + installed-model
  count, so it can't drift.

```ts
export type OnboardingStep = "ollama" | "model" | "ready";
export function currentStep(ollamaHealthy: boolean | null, modelCount: number): OnboardingStep {
  if (ollamaHealthy !== true) return "ollama"; // 1. start the engine
  if (modelCount === 0)        return "model";  // 2. install a model
  return "ready";                               // 3. scaffold a workspace
}
```

### `components/OnboardingCoach.tsx` — the card
- **Responsibility:** render the right step's CTA and run the "finish" action.
- **How/Where used:** mounted in `App.tsx` above the nav. Reads `ollamaHealthy`
  (`backendStore`) + `list.length` (`installedModelsStore`), passes them to
  `currentStep`. Step CTAs:
  - **ollama** → embeds the shared `OllamaEmptyState` (start/install buttons).
  - **model** → `pullModel(RECOMMENDED_MODEL)` (`llama3.2:1b`) then jumps to the
    `downloads` view; or "Browse models" → `models` view.
  - **ready** → `openWorkspace()`: `scaffoldOnboardingWorkspace()` (Rust creates
    `~/Documents/QuantaMind` + a `welcome.quantamind.yaml`), opens it in
    `workspaceStore`, selects the welcome prompt, then `finish()`.
  - **Skip setup** at any step → `finish()` (persists the gate, no scaffold).

`scaffoldOnboardingWorkspace`/`pullModel`/`RECOMMENDED_MODEL` live in
`shared/ipc/system/onboarding.ts`.

---

## Help + Updater (tab + startup overlay)

The Help tab is a documented index of every page/metric; bolted onto its top is
the manual update checker. A separate always-mounted overlay does the automatic,
once-per-day background check.

### Help page (presentational — compact)

| File | What it is |
|---|---|
| `help/components/helpSections.tsx` | The content: a `HELP_SECTIONS: HelpSection[]` array of What/Why/How blocks (sections: workspace, analysis, inspector, models, downloads, eval, …). Computed metrics also carry a `formula` + `source` file so derivations are visible, never hand-waved. ~489 lines of data; no logic. |
| `help/components/HelpSidebar.tsx` | Left rail; one button per section; highlights `activeId`; calls `onSelect`. |
| `help/components/HelpContent.tsx` | Center pane; renders the active section's blocks as What/Why/How cards (+ optional `formula`/`source`). Each block gets a `#help-<section>-<block>` anchor id. |
| `help/components/HelpPage.tsx` | Hosts sidebar + content + the `UpdateChecker`. Reads a `#help-<section>` URL hash to deep-link from other pages (e.g. a CSV importer's "learn more"), scrolling to the named block; re-applies on `hashchange`. |

### `updateSchedule.ts` — the 24h gate (pure)
- **Responsibility:** decide whether a background check is due. Treats "never
  checked" and an unparseable timestamp as due.

```ts
export const DAY_MS = 24 * 60 * 60 * 1000;
export function shouldCheck(last: string | null | undefined, nowMs: number): boolean {
  if (!last) return true;
  const t = Date.parse(last);
  if (Number.isNaN(t)) return true;
  return nowMs - t >= DAY_MS;
}
```

### `hooks/useUpdater.ts` — manual check + install state machine
- **Responsibility:** drive the Help tab's update checker — a small state
  machine over the Tauri updater plugin.
- **What:** statuses `idle → checking → up_to_date | available → downloading →
  installing | error`. Tracks `currentVersion`, the `Update` handle, and
  download progress (`downloaded`/`total`).
- **How:** loads the running version on mount; `check()` calls the plugin and
  classifies the result; `install()` streams progress and flips to `installing`
  once the download completes (relaunch happens inside `downloadAndInstall`).

```ts
const check = useCallback(async () => {
  setStatus("checking");
  try {
    const found = await checkForUpdate();          // plugin-updater `check()`
    if (found) { setUpdate(found); setStatus("available"); }
    else       { setUpdate(null);  setStatus("up_to_date"); }
  } catch (e) { setError(formatIpcError(e)); setStatus("error"); }
}, []);

const install = useCallback(async () => {
  if (!update) return;
  setStatus("downloading");
  try {
    await downloadAndInstall(update, (d, t) => {   // streams chunk progress
      setDownloaded(d); setTotal(t);
      if (t && d >= t) setStatus("installing");
    });
  } catch (e) { setError(formatIpcError(e)); setStatus("error"); }
}, [update]);
```

The IPC wrapper (`shared/ipc/system/updater.ts`) calls
`@tauri-apps/plugin-updater`'s `check()` (returns `Update | null`), sums the
`Started`/`Progress` events from `update.downloadAndInstall(...)`, then calls
`@tauri-apps/plugin-process` `relaunch()`.

### `components/UpdateChecker.tsx` (Help tab)
- Presentational view over `useUpdater`: "You're on vX", a "Check for updates"
  button, and per-status banners (up-to-date / available with release notes via
  shared `Markdown` / downloading % / installing / error). Never auto-installs —
  install is an explicit button.

### `components/StartupUpdate.tsx` — automatic background check (overlay)
- **Responsibility:** at most one background update check per 24h on launch;
  shows a consent banner if a newer version exists. **Never auto-installs.**
- **How:** on mount reads `last_update_check_at`; if `shouldCheck` is false,
  returns. Otherwise `checkForUpdate()`, **stamps `last_update_check_at` now**
  (so "Remind me later" defers a full day without re-prompting), and shows the
  banner only if an update was found. "Install now" → `downloadAndInstall`
  (relaunches).

```tsx
useEffect(() => { void (async () => {
  const s = await getUserSettings();
  if (!shouldCheck(s.last_update_check_at, Date.now())) return;
  const found = await checkForUpdate();
  await setUserSettings({ ...s, last_update_check_at: new Date().toISOString() });
  if (found) setUpdate(found);
})(); }, []);
```

---

## Feedback (overlay: button + modal)

A low-friction channel to email feedback to `info@quantamind.co` via the user's
own mail client — no backend, no network call from the app.

| File | What it is |
|---|---|
| `feedback/components/FeedbackButton.tsx` | Fixed bottom-right pill; opens the modal. (`HistoryPanel`/`StartupUpdate` sit at other corners.) |
| `feedback/components/FeedbackModal.tsx` | The form: a 10–5000 char message, an opt-in "include diagnostics" checkbox (app version / OS / current model), Esc-to-close, char counter, error state. On success shows a toast ("Opened your mail app…") and closes. Reads current model from `selectedModelStore`. |

### `hooks/useSubmitFeedback.ts`
- **Responsibility:** build the `mailto:` URL and hand it to the OS.
- **How:** `submit()` calls `buildFeedbackMailto(input)` then opens it with
  `@tauri-apps/plugin-shell`'s `open` (launches the default mail client with a
  pre-filled draft). Returns `true`/`false`; the modal toasts + closes on `true`.

```ts
const submit = useCallback(async (input: FeedbackInput): Promise<boolean> => {
  setStatus("opening");
  try {
    await openExternal(buildFeedbackMailto(input)); // plugin-shell open(mailto:)
    setStatus("success"); return true;
  } catch (e) { setError(formatIpcError(e)); setStatus("error"); return false; }
}, []);
```

`buildFeedbackMailto` (`shared/ipc/system/feedback.ts`) appends an opt-in
diagnostics block (`App: QuantaMind vX`, `User-Agent`, `Model`) and
URL-encodes subject+body. Constants: `MIN_MESSAGE_LEN=10`, `MAX=5000`,
`FEEDBACK_TO=info@quantamind.co`.

---

## History (header-toggled drawer overlay)

Every completed run is recorded to **per-workspace** history and is replayable.
The drawer is toggled from a "History" button in `AppHeader`
(`useHistoryStore.toggle`), not from the tab nav.

### `recordRun.ts` — record a finished run
- **Responsibility:** persist a completed run, then refresh the panel if open.
- **Why:** best-effort — a failure (e.g. no workspace open) is logged, never
  surfaced as a run error. Called from `workspace/hooks/useStreamingRun.ts`
  after a stream completes.
- **How:** maps the run context + metrics to `historyAppend`'s `AppendArgs`.

```ts
export async function recordRun(ctx: RunContext | null, output: string, metrics: RunMetrics) {
  if (!ctx || !output) return;                 // nothing to record
  try {
    await historyAppend({
      name: ctx.name ?? "", prompt_path: ctx.promptPath ?? null,
      model: ctx.model, system: ctx.system ?? "", user: ctx.prompt,
      params: ctx.params ?? {}, output,
      token_count: metrics.token_count,
      ttft_ms: metrics.ttft_ms ?? null,
      tokens_per_sec: metrics.tokens_per_sec ?? null,
      load_ms: metrics.load_ms ?? null,
    });
    if (useHistoryStore.getState().open) await useHistoryStore.getState().load();
  } catch (e) { console.error("history append failed:", e); }
}
```

The on-disk shape (`shared/ipc/workspace/history.ts`, validated by zod):
`HistoryEntry` = `id, name, prompt_path?, model, system, user, params,
output_preview, output_len, token_count, ttft_ms?, tokens_per_sec?, load_ms?,
ran_at`. The Rust side spills full output to a blob and keeps a preview in the
index (see [`backend-prompt-workspace-system.md`](backend-prompt-workspace-system.md)).

### `state/historyStore.ts` — drawer + list state
- **Responsibility:** `open` (drawer visibility), `entries`, and the actions
  `toggle`/`setOpen`/`load`/`clear`.
- **How:** `load` → `historyList()` into `entries`; `clear` → `historyClear()`
  then empties the list. (`FilesSection` also calls `historyRemoveByPath` when a
  prompt file is deleted, to prune its orphaned history rows.)

```ts
export const useHistoryStore = create<HistoryStoreState>((set) => ({
  open: false, entries: [],
  toggle: () => set((s) => ({ open: !s.open })),
  setOpen: (open) => set({ open }),
  load:  async () => set({ entries: await historyList() }),
  clear: async () => { await historyClear(); set({ entries: [] }); },
}));
```

### `components/HistoryPanel.tsx` — the drawer + replay
- **Responsibility:** render the right-side drawer; reload on open; restore a
  past run into global state.
- **How:** `restore(e)` rehydrates a run into a **detached draft** plus the
  header controls: `restoreDraft` (name/user/system/model into `workspaceStore`),
  `globalParams ← e.params` (`paramsStore`), and re-selects the model — switching
  the backend (`backendStore`) when the model is installed — via
  `selectedModelStore`, then closes the drawer.

| File | What it is |
|---|---|
| `history/components/HistoryRow.tsx` | One clickable row: title (`name`/first 80 chars of user), localized `ran_at`, and `<model-label> · N chars · M tokens`. Click → `onRestore`. |

---

## Audit (tab)

`audit/components/AuditPage.tsx` is "Zone 2 — the compliance home." It does
**not** show run history; it shows benchmark/regression artifacts:

1. **Context Stress Test** (`ContextCliffPanel`) at the top — a diagnostic
   moved off the Tests workspace.
2. **Audit & Compliance — Results History:** a collection picker (presets +
   custom collections from `evalRegistryStore`) feeding `loadCollectionHistory`,
   rendered as a `HistoryTimeline`. **Filtered to the selected backend** so a
   backend switch never shows the previous backend's runs. History is re-fetched
   on collection change **and** when a batch finishes for the shown collection — an
   effect keyed on `batchStore.report` re-reads the on-disk history once
   `report.collection_id === collection`, so the graph updates live instead of
   needing an app restart (the collection-id guard stops another collection's run
   from clobbering the chart).
3. **Export Audit Trail:** CSV (`batchToCsv`) and JSON (`JSON.stringify`) export
   of the latest batch `report` from `batchStore` (both disabled until a report
   exists). An `InfoButton` links the in-context help.

It reads from the eval feature's stores/IPC (`evalRegistryStore`, `batchStore`,
`shared/ipc/eval/matrix`); see the eval documentation for the underlying matrix
contract.

---

## Out of scope (cross-reference only)

`GlobalControls.tsx`, `ParamsControl.tsx`, `ModelSelector.tsx`, and
`BackendSelector.tsx` are part of the app shell and are documented in
[`frontend-overview.md`](frontend-overview.md). History's `restore` writes into
those shells' stores (`paramsStore`, `selectedModelStore`, `backendStore`); the
Onboarding coach reads `backendStore`/`installedModelsStore`.

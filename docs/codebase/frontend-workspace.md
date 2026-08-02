# Frontend — Workspace Tab & Workspaces File Tree

The **Workspace** is the primary single-prompt page: a Monaco prompt editor + model
picker + run controls that **stream tokens** back from the selected local backend
(llama.cpp / vLLM / SGLang), reporting TTFT and tok/s, behind an explicit
`running → streaming → done | cancelled | error` state machine with clean
cancellation. It also hosts per-backend **server start/stop** controls and a left-rail
**file tree** of YAML prompts with autosave.

> Cross-references:
> - Backend that serves `run_prompt` / `stop_prompt` + prompt/workspace persistence → [`backend-prompt-workspace-system.md`](./backend-prompt-workspace-system.md)
> - Backend that starts/stops the three inference servers + health → [`backend-inference-backends.md`](./backend-inference-backends.md)
> - Shared IPC client, Zustand stores, navigation → [`frontend-overview.md`](./frontend-overview.md)

---

## Overview

### Why it's the core tab
Everything else (Analysis, Models, History, Tests) feeds off the act of running a prompt
against a local model. The Workspace is where that single run happens. The global header
owns model selection and the per-backend Play/Stop; this page owns the **prompt content,
the Run trigger, the live stream, and the per-prompt file**.

### What it does
- Renders two Monaco editors: an optional **system** prompt and the **user** prompt.
- Picks up the model(s) selected in the global header. **1 model → a single streaming
  run** (`SingleRun`); **2+ models (llama.cpp) → a compare** (`MultiRun`, documented under
  Compare). The single run's live output is mirrored into `compareStore` and shown on the
  **Analysis** tab — the Workspace navigates there on Run.
- Streams tokens into that output pane and, on completion, records final metrics
  (TTFT, tok/s, token count, load_ms) into history + the StatusBar.
- Exposes a per-backend **server Play/Stop** in the header area and, when **no LLM
  backend is healthy**, replaces the editor with a `BackendSetupGuide`.
- Left rail: open a workspace folder, browse a tree of `*.quantamind.yaml` prompts,
  create / rename / delete them, with **debounced autosave**.

### How — IPC commands used (via `shared/ipc`)
| Command | Wrapper / hook | Backend doc |
| --- | --- | --- |
| `run_prompt` (emits `prompt-token` / `prompt-done` / `prompt-cancelled` events) | `useStreamingRun` | backend-prompt-workspace-system.md |
| `stop_prompt` | `useStreamingRun.cancel` (5s-bounded) | backend-prompt-workspace-system.md |
| `start_llama_cpp` / `stop_llama_cpp` | `useStartllama.cpp` / `useStopllama.cpp` | backend-inference-backends.md |
| `start_llama_server` / `stop_llama_server` | `useStartLlamaServer` / `useStopLlamaServer` | backend-inference-backends.md |
| `start_vllm_server` / `stop_vllm_server` / `vllm_server_status` | `useVLlmServer` | backend-inference-backends.md |
| llama.cpp / vLLM / SGLang `*_health` | `StatusBar`, `useLlamaBackend`, `useVLlmBackend` (5s poll) | backend-inference-backends.md |
| `open_workspace` / `close_workspace` / `list_workspace_tree` / `recent_workspaces` | `workspaces` store + hooks | backend-prompt-workspace-system.md |
| `create_prompt` / `load_prompt` / `save_prompt` / `rename_path` / `delete_path` | `workspaces` store + hooks | backend-prompt-workspace-system.md |
| `list_prompt_templates` | `PromptTemplatePicker` | backend-prompt-workspace-system.md |

Architecture note: **a backend is coupled to the model's weight format** — there is no
health fallback. If the active backend isn't healthy, Run is blocked with a "start it"
hint (`runHint.ts`).

---

## The page — `workspace/components/Workspace.tsx`

**Responsibility:** the run surface; picks which top-level mode to render and
composes the editors + run controls.

**Why:** one component decides the page shape from global state (any LLM
running? a prompt selected? one model or many?) so the sub-pieces stay dumb.

**What / How:** reads `useWorkspacesStore.current` (the open prompt) + `patch`, the
header `selectedModels`, and the per-backend health flags from `backendStore`
(llama_cpp/llama/vllm/vllm/sglang).

Render priority:
1. `noLlmRunning` (all health flags `!== true`) → `<BackendSetupGuide/>`.
2. otherwise the prompt UI: `ModelSelectBar`, then — if a prompt is open — the system
   `PromptEditor` (120px), the user-prompt label + `PromptTemplatePicker`, the user
   `PromptEditor`, then `multi ? (HardwareSummary + RunStrategyPicker + MultiRun) : <SingleRun/>`.
4. `<StatusBar/>` is **always mounted** (even under the setup guide) so its llama.cpp
   health poll keeps running and can flip the page back to the editor.

```tsx
const noLlmRunning = useBackendStore(
  (s) => s.llama_cppHealthy !== true && s.llamaHealthy !== true && s.vllmHealthy !== true,
);
const sttRunning = useSttRuntimeStore(runningSttEngine);
const multi = selectedModels.length >= 2;
const model = selectedModels[0]?.name ?? null;
// sttRunning ? SttWorkspace : noLlmRunning ? BackendSetupGuide : <editor + (multi?MultiRun:SingleRun)>
```

---

## The run engine — `workspace/hooks/useStreamingRun.ts`

**Responsibility:** own one streaming run end-to-end — invoke `run_prompt`, consume the
global token/done/cancelled event stream, drive the status machine, and on completion
fan out metrics to the workspace store, history, and the leak inspector.

**Why:** the `run_prompt` event stream is **global** (other hooks may also
listen). An `initiatedRef` flag gates every handler so this hook reacts
**only to the run it started** — no stray history rows, leak samples, or compare writes
for someone else's run. There is intentionally **no timeout** on `run_prompt` (model
loads + long generations are legitimate); cancellation is the user's escape hatch and is
bounded at `STOP_PROMPT_TIMEOUT_MS = 5_000`.

**What / How:** every event payload is Zod-validated (`TokenPayloadSchema`,
`DonePayloadSchema`, `CancelledPayloadSchema`); an invalid payload trips `fail()` →
`status="error"` with `"invalid backend payload"`. Listeners are registered once in a
mount effect with a `cancelled` race-guard and torn down on unmount.

```ts
export type RunStatus = "idle" | "running" | "done" | "cancelled" | "error";

// token: append to both a ref (durable) and state (renders)
listen(EVENT_TOKEN, (e) => {
  if (!initiatedRef.current) return;
  const p = TokenPayloadSchema.safeParse(e.payload);
  if (!p.success) return fail("prompt-token", p.error.issues);
  outputRef.current += p.data.text;
  setOutput((prev) => prev + p.data.text);
});

// done: terminal → metrics + history + leak sample
listen(EVENT_DONE, (e) => {
  if (!initiatedRef.current) return;
  const p = DonePayloadSchema.safeParse(e.payload); if (!p.success) return fail(...);
  initiatedRef.current = false;
  setMetrics(p.data); setStatus("done");
  useWorkspaceStore.getState().setLastRunMetrics(p.data);
  void recordRun(ctxRef.current, outputRef.current, { token_count, ttft_ms, tokens_per_sec, load_ms });
  void useLeakStore.getState().sample(ctxRef.current?.model ?? "");
});

// cancelled: terminal, carries partial token_count
listen(EVENT_CANCELLED, (e) => { ...; initiatedRef.current = false; setCancelledInfo(p.data); setStatus("cancelled"); });

const start = useCallback(async (model, prompt, system?, params?, promptPath?, name?) => {
  setOutput(""); setMetrics(null); setCancelledInfo(null); setError(null);
  outputRef.current = ""; ctxRef.current = { name, model, prompt, system, params, promptPath };
  initiatedRef.current = true; setStatus("running");
  const args: Record<string, unknown> = { model, prompt };
  if (system?.trim()) args.system = system.trim();
  if (hasParam(params)) args.params = params;
  args.backend = useBackendStore.getState().selectedBackend;
  if (useParamsStore.getState().keepLoaded) args.keepAlive = -1; // llama.cpp keep_alive=-1 → resident
  try { await invoke("run_prompt", args); }
  catch (e) { initiatedRef.current = false; setError(formatIpcError(e)); setStatus("error"); }
}, []);

const cancel = useCallback(async () => {
  try { await withTimeout(invoke("stop_prompt"), STOP_PROMPT_TIMEOUT_MS, "stop_prompt"); }
  catch { /* best-effort: backend may have already finished */ }
}, []);
```

Notes:
- `keepLoaded` on → `keepAlive=-1` (llama.cpp resident). Off → the arg is **omitted** (not
  `0`), so llama.cpp's idle-unload lets the model linger for the Latency tab before freeing.
- `cancel()` doesn't itself set a state — the backend emits `prompt-cancelled`, and the
  listener transitions to `cancelled`. If the run already finished, the catch swallows it.
- Returns `{ output, status, error, metrics, cancelledInfo, start, cancel }`.

### Run state machine

| From → event | To | Side effect |
| --- | --- | --- |
| `idle` → `start()` | `running` | reset output/metrics; `initiated=true`; `invoke run_prompt` |
| `running` → `prompt-token` | `running` (streaming) | append token to output |
| `running` → `prompt-done` | `done` | set metrics; `setLastRunMetrics`; `recordRun`; leak `sample` |
| `running` → `prompt-cancelled` | `cancelled` | store partial `token_count` |
| `running` → `run_prompt` throws | `error` | `setError(formatIpcError)` |
| any → invalid payload | `error` | `"invalid backend payload"` |
| `done`/`cancelled`/`error` → `start()` | `running` | fresh run (clears prior result) |

```
idle ──start──► running ──token──► running (loop)
                  │                  │
                  ├──done───────────► done
                  ├──cancelled──────► cancelled   (Cancel button → stop_prompt)
                  └──throw / bad payload ► error
```

---

## Data-flow walkthrough — one run

1. **Type prompt** → Monaco `onChange` → `useWorkspacesStore.patch({ user })` (marks
   `dirty`; autosave debounces a `save_prompt`).
2. **Pick model** in the global header → `selectedModels[0]` → `model` prop on `SingleRun`;
   `backendStore.selectedBackend` is the model's required backend.
3. **Click Run** (or Cmd+Enter) in `RunControls` → `SingleRun.runNow()`:
   - guards `canRun = model && prompt.trim() && !blockedHint`;
   - **server ensured up**: `backendRunHint` returns a string if the active backend is
     unhealthy → Run is disabled with that hint; the user presses ▶ in the header
     (`ServerControl`) to start it.
   - `setTopView("compare")` (navigate to Analysis) then
     `start(model, prompt, system, globalParams, currentPath, name)`.
4. `run_prompt` invoked with `{ model, prompt, system?, params?, backend, keepAlive? }`.
5. **Token events stream** into `output`; `SingleRun`'s effect mirrors
   `{ model, status, output, metrics, error }` into `compareStore.setSingleRun` on every
   change → the **Analysis tab renders the live output pane**.
6. **`prompt-done`** → `status="done"`, metrics fan out (StatusBar shows
   `TTFT …ms · … tok/s · N tokens`, history records the run, leak inspector samples). An
   unsaved draft is persisted via `saveDraftAuto()`.

---

## `workspace/components/run/` — run controls

### `SingleRun.tsx`
**Responsibility:** single-model run trigger wiring `useStreamingRun` to the open prompt,
header model, params, backend health, hotkeys, the Analysis mirror, and draft autosave.
**Why/How:** computes `blockedHint` from `backendRunHint(activeBackend, {llama_cpp,llama,vllm})`
and `canRun`; on Run navigates to `compare` and calls `start(...)`. Two effects:
`status==="done" → saveDraftAuto()`, and mirror-into-compareStore on every output/status
change. Wires `useWorkspaceHotkeys` (Run/Stop/Save, gated to the active workspace view).

```tsx
const blockedHint = backendRunHint(activeBackend, { llama_cpp: llama_cppHealthy, llama: llamaHealthy, vllm: vllmHealthy });
const canRun = !!model && prompt.trim().length > 0 && !blockedHint;
const runNow = () => { if (!model) return;
  useNavStore.getState().setTopView("compare");
  void start(model, prompt, system, useParamsStore.getState().globalParams, currentPath, current?.name);
};
```

### `RunControls.tsx`
**Responsibility:** presentational Run/Cancel buttons + status text.
**How:** `runDisabled = running || !canRun || blocked`; surfaces `blockedHint` as the
button `title` and an amber inline note; Cancel enabled only while `running`; shows the
raw `status` string (`data-testid="run-status"`).

---

## `workspace/components/model-select/` — model bar

| File | Responsibility |
| --- | --- |
| `ModelSelectBar.tsx` | Workspace model affordances. The real picker now lives in the **global header**; this keeps the `llama.cppEmptyState` (when llama.cpp is the active backend and down) and an **Add Model** shortcut to the Models tab. |
| `ModelPicker.tsx` | Legacy inline picker (header now owns selection): de-dupes installed models by digest, filters out embedding models, a `<select>`, a llama.cpp **Stop** square, **Add Model**; re-`refresh()`es installed models when llama.cpp flips healthy. |
| `ModelTemperaturePopover.tsx` | Per-model temperature popover (gear icon). Range 0–2, commits on pointer/key-up to `modelSettingsStore.setTemperature(model, v)`; outside-click + Escape close; **Reset to `DEFAULT_TEMPERATURE`**. Disabled until a model is picked. |

---

## `workspace/components/prompt/` — editor & params

### `PromptEditor.tsx`
**Responsibility:** the Monaco editor wrapper used for both system and user prompts.
**How:** `@monaco-editor/react` `Editor`, `language="markdown"`, `theme="vs-dark"`,
minimap off, `wordWrap:"on"`, font 13, `scrollBeyondLastLine:false`; `onChange` coerces
`undefined → ""`. Height + `testId` are props (120px system, 240px default user).

| File | Responsibility |
| --- | --- |
| `ParamRow.tsx` | One inference-param row: a slider + number input + reset (↺). Empty input → `undefined` (use model default); parses int/float per `info.integer`; slider falls back to the placeholder default when unset. |
| `paramsInfo.ts` | `PARAMS[]` display metadata + tooltips for temperature, top_p, top_k, max_tokens, repeat_penalty, seed, num_ctx. Ranges mirror backend `commands/prompt_options.rs`; placeholders show effective defaults. `num_ctx` drives the launch `-c` for llama.cpp too (changing it restarts the server, since llama.cpp fixes context at spawn); llama.cpp applies it per run; vLLM is fixed by the model. |

---

## `workspace/components/status/` — health + server controls

`StatusBar` is the only non-trivial piece; the rest are thin Play/Stop wrappers.

### `StatusBar.tsx`
**Responsibility:** fixed footer showing the model name, a backend **health dot + label**,
and the last run's metrics. **Why:** it owns the 5s llama.cpp-health poll (writes
`setllama.cppHealthy`) — the heartbeat that flips the page between setup guide and editor.
**How:** polls `checkllama.cppHealth` every `POLL_MS=5000`; derives the dot/label from
`backendStatus(activeBackend, health, llamaHealthy, vllmHealthy, model)` so the status
reflects the **active** backend, not always llama.cpp; metrics via `formatMetrics`.

| File | Responsibility |
| --- | --- |
| `ServerControl.tsx` | Dispatches the single header control by `selectedBackend`: `llama.cppControl` / `VLlmServerControl` / `LlamaServerControl` for the local backends, or `RemoteServerControl` (read-only status — no start/stop) for the remote vLLM/SGLang. |
| `RemoteServerControl.tsx` | Read-only header status for the remote vLLM/SGLang backends (health dot + "configure in Settings" hint); the app can't start a remote server. Health comes from `useRemoteBackends` polling into `backendStore.vllmHealthy`/`sglangHealthy`. |
| `llama.cppControl.tsx` | `PlayStopButton` over `useStartllama.cpp` / `useStopllama.cpp`; hidden until health known (`null`). |
| `LlamaServerControl.tsx` | Play/Stop the `llama-server` sidecar on the selected llama.cpp model's GGUF (`model.path`); disabled with no path. A start error or hardware-constraint note (from `useStartLlamaServer`) renders as a compact ⚠ chip (`LlamaStartBadge`, folded into the file) — the full text opens in a hover popover (auto-shown once per new message, then hover-only) so the long note can't crush the header row. Error chip wins over the notice chip. |
| `VLlmServerControl.tsx` | Play/Stop the app-managed `vllm_lm.server` on the selected vLLM model's dir; the busy spinner covers the multi-minute first-run weight load. |
| `llama.cppEmptyState.tsx` | llama.cpp-down recovery card: Start / Install (opens download page) / Retry, with `starting` / `success` / `not_installed` / `error` states. |
| `backendStatus.ts` | Pure: dot+label+aria per backend — llama.cpp names its version; llama.cpp name the loaded model and their run state. |

---

## `workspace/hooks/` — server control & health

The server-control hooks all follow the same shape: a small status enum, call the IPC
wrapper, write the matching `backendStore.set*Healthy`, surface a formatted error. **They
never block Run on their own success** — health polling is the source of truth.

| Hook | IPC | Status enum | Notes |
| --- | --- | --- | --- |
| `useStartllama.cpp` | `start_llama_cpp` | `idle/starting/success/error/not_installed` | on `not_installed` captures `install_url`; `openInstallPage` opens it; success lingers 1s then flips `llama_cppHealthy=true` + refreshes installed models. |
| `useStopllama.cpp` | `stop_llama_cpp` | `idle/stopping/error` | sets `llama_cppHealthy=false`. |
| `useStartLlamaServer` | `start_llama_server(path)` | `idle/starting/success/error/not_bundled` | one GGUF at a time; `already_running`/`started` → `llamaHealthy=true`. |
| `useStopLlamaServer` | `stop_llama_server` | `idle/stopping/error` | sets `llamaHealthy=false`. |
| `useLlamaBackend` | `*_health` poll (5s) | — | re-probes llama health so a died server doesn't stay "healthy"; no Apple-Silicon gate. |
| `useVLlmBackend` | hardware snapshot + `vllm_health` poll (5s) | — | detects Apple Silicon (only platform vLLM runs on); polls only there. Returns `{ appleSilicon }`. |
| `useRemoteBackends` | `check_vllm_health` / `check_sglang_health` poll (5s) | — | `useVllmBackend`/`useSglangBackend` (one file, shared poll helper) write `vllmHealthy`/`sglangHealthy`; false until the endpoint is configured in Settings and reachable. No start/stop hooks — the servers are remote. |
| `useWorkspaceHotkeys` | — | — | Cmd+Enter Run, Cmd+. Stop, Cmd+S Save, gated by `active`/`canRun`/`running`/`hasPrompt`. |

### `useVLlmServer.ts` (the interesting one)
**Responsibility:** start/stop the app-managed `vllm_lm.server`, polling status + health so a
**multi-minute first-run download** shows "Downloading weights…" without ever failing by
timeout, and a **died process surfaces its stderr tail** instead of spinning forever.
**How:** `start` returns immediately, then a `POLL_MS=1500` interval runs `poll()` until
health goes available (`settle(true)`) or status reports `exited` (`settle(false, stderr_tail)`);
`running` updates the phase label (`downloading`/`starting`). Start failures map
`not_found` / `no_free_port` / `start_failed` to specific messages.

```ts
const poll = useCallback(async () => {
  if ((await checkVLlmHealth()).available) return settle(true, null);
  const st = await vllmServerStatus();
  if (st.state === "exited") settle(false, st.stderr_tail || `vllm_lm.server exited (code ${st.code ?? "?"})`);
  else if (st.state === "running") setPhase(st.phase === "downloading" ? "downloading" : "starting");
}, [settle]);
```

---

## `workspace/state/` — stores & helpers

### `workspaceStore.ts` (run metrics)
**Responsibility:** holds **only** the last run's final metrics so the StatusBar can show
them after a run. Backend selection + server health live in `shared/state/backendStore`;
per-action state lives in the hooks.

```ts
export interface WorkspaceStore {
  lastRunMetrics: DonePayload | null;
  setLastRunMetrics: (m: DonePayload) => void;
}
```

`DonePayload` (from `shared/ipc/events/events.ts`, all timing fields nullable —
null = "not measured", never 0): `{ ttft_ms, tokens_per_sec, token_count, timeline[],
stats? }`.

| File | Responsibility |
| --- | --- |
| `runHint.ts` | `backendRunHint(backend, health)` → the Run-block string when the **model's required** backend isn't healthy. No fallback: llama_cpp→"Start llama.cpp first", llama_cpp→"Start llama.cpp to run this model", vllm→"Start the vLLM backend…", vllm/sglang→"Set the vLLM/SGLang server URL in Settings and start it". |
| `format.ts` | `formatMetrics(DonePayload)` → `"TTFT {ttft}ms · {tps} tok/s · {n} tokens"` (em-dash when null). |

---

## `workspace/components/` — top-level helpers

| File | Responsibility |
| --- | --- |
| `BackendSetupGuide.tsx` | Shown when no LLM backend is healthy: a 2-col grid of install cards (llama.cpp / vLLM / SGLang) with copy-able commands, links, step lists, and "what it runs". `useVLlmBackend` filters the Apple-only vLLM card off non-Apple-Silicon. The page swaps back to the editor the instant the health poll sees a server. |
| `PromptTemplatePicker.tsx` | A `<select>` of bundled prompt templates (`list_prompt_templates`); picking one calls `onInsert(t.body)` → `patch({ user })`. Renders nothing when empty. |

---

## `workspaces/` — the file tree + persistence

### `state/workspaceStore.ts` (the workspace/prompt store)
**Responsibility:** the open folder, its tree, the selected/open prompt, the dirty flag,
and every persistence action. This is the store `Workspace.tsx` reads `current` from and
`patch`es into.

**Shape & key actions:**
```ts
interface WorkspaceStoreState {
  root: string | null; tree: TreeNode[];
  currentPath: string | null; current: PromptFile | null; dirty: boolean;
  open(path); close(); refreshTree(); selectPrompt(path); clearSelection();
  patch(p: Partial<PromptFile>);   // marks dirty; no-op if nothing open
  save();                          // save_prompt(currentPath, current) → dirty=false
  restoreDraft(fields);            // History/restored content as a DETACHED draft (currentPath=null)
  saveAs(name); saveDraftAuto();
}
```
- `patch` sets `dirty=true` (autosave debounces a `save`).
- `restoreDraft` loads history content with `currentPath=null` so it **never overwrites
  the open file** until the user saves.
- `saveDraftAuto` (called by `SingleRun` after a successful run) persists an unsaved draft
  into the workspace, auto-named + deduped via `uniqueName` (`base`, `base-2`, …).

`PromptFile`: `{ name, system, user, model, params?, created_at, updated_at, auto_rerun }`.
`TreeNode`: a file `{ kind:"file", name, path }` or folder `{ kind:"folder", name, path, children }`.

### Components

| File | Responsibility |
| --- | --- |
| `WorkspaceSwitcher.tsx` | The folder dropdown: shows the current root's basename; "Open folder…" (`browse`) + a list of `recent_workspaces` (`openPath`). |
| `FilesSection.tsx` | The "Files" rail: **+ New** inline input (`useCreatePrompt`), empty/no-root hints, and renders `FilesTree`. Owns **delete** — confirms via `ask()`, `delete_path`, clears selection if it was open, refreshes tree, and prunes History (`historyRemoveByPath` + `historyStore.load`). |
| `FilesTree.tsx` | Recursive tree: folders are collapsible (`FolderRow`, open by default, `depth*12` indent); files render `FileRow`. |
| `FileRow.tsx` | One prompt row: strips `.quantamind.yaml`, click → select, double-click → inline rename, hover ✕ → delete; highlights the active file. |

### Hooks

| Hook | IPC | Responsibility |
| --- | --- | --- |
| `useAutoSave` | `save_prompt` (via store `save`) | Mount once. 500ms-debounced save after the last `dirty` change; a `seq` race-guard discards stale saves on selection change; a failed save **toasts** (never silently dropped). |
| `useCreatePrompt` | `create_prompt` | Shared "+ New" / Cmd+N flow: name from caller (no `window.prompt` in the webview), create → refresh → select; toasts on failure or no workspace. |
| `useOpenWorkspace` | `open_workspace` (store `open`) | `browse()` (directory dialog) + `openPath(path)` (from recents); toasts on failure. |
| `useRenamePrompt` | `rename_path` | Keep the dir + `.quantamind.yaml` suffix, swap the base name; refresh + re-select if open; rejects empty/no-op names; toasts collisions. |

---

## The sidebar shell — `WorkspaceSidebar.tsx`

**Responsibility:** the Workspace's single left rail, **composed at the shell level**
(features don't import each other). When `uiStore.sidebarVisible` is off, renders just a
`›` expand button; otherwise an `<aside>` (w-64) with a hide button, `WorkspaceSwitcher`,
then `FilesSection`. Backend selection moved to the global header, so the sidebar is now
folder + files only.

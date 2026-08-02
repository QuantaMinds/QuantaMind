# Frontend — Models & Downloads tabs

> Feature root: `frontend/src/features/models/`
> Backend contracts: [`backend-models-hf-gguf.md`](./backend-models-hf-gguf.md) (HF search/install, GGUF inspect, llama.cpp pull, vLLM),
> storage commands in [`backend-prompt-workspace-system.md`](./backend-prompt-workspace-system.md).
> Shared helpers: [`frontend-overview.md`](./frontend-overview.md) (`shared/models/modelLabel`, IPC layer).
> Registry consumers: [`frontend-workspace.md`](./frontend-workspace.md), [`frontend-compare-analysis.md`](./frontend-compare-analysis.md).

## Overview

**Why a terminal-free installer.** QuantaMind is a desktop app for running local LLMs.
The whole point is that a non-CLI user never has to type `llama-server`, clone a HF repo,
or hand-edit a chat-template config. Every install path — Hugging Face, a Hugging Face GGUF
variant, a local `.gguf` on disk, an vLLM snapshot — is a click with a live progress bar,
a guardrail against models that can't run, and a fit-in-memory hint *before* you pull
gigabytes. Failures surface as plain-language guidance ("this GGUF is truncated, re-download
the full file"), never a raw parser dump.

**What the two tabs do.** They are two top-level nav views (`navStore.topView`), not
sub-tabs of one page:

- **Models** (`ModelsPage.tsx`) = *browse / install / inspect*. Three sub-tabs:
  llama.cpp Library, Hugging Face, Local File. This is where you find and pull a new model.
- **Downloads** (`DownloadsPage.tsx`) = *active + installed + storage*. Shows in-flight
  downloads (cancellable), the grouped installed-model list (delete, add-to-llama.cpp), and
  storage controls (paths, disk usage, clear-cache).

**How (IPC surface).** Every action is a Tauri command; live progress rides Tauri events.

| Source / surface | Hook | IPC command(s) | Live event |
|---|---|---|---|
| llama.cpp Library | `useModelInstall` | `pull_model` → pullId, `cancel_pull` | `pull-progress` |
| Hugging Face GGUF | `useHfInstall` | `hf_search`, `hf_repo_files`, `hf_model_card`, `install_hf_gguf`, `cancel_hf_install` | `hf-progress` |
| Hugging Face vLLM | `useVLlmInstall` | `hf_search`, `hf_repo_all_files`, `hf_model_card`, `install_vllm_model`, `cancel_hf_install` | `hf-progress` |
| Local File | `useLocalImport` | `inspect_gguf`, `install_local_gguf` | `local-install-progress` |
| Add-to-llama.cpp (installed llama.cpp) | `AddTllama_cpp.cppButton` | `install_local_gguf` | `local-install-progress` |
| Installed list (all backends) | `installedModelsStore.refresh` | `get_installed_models_with_stats`, `list_llama_models`, `list_vllm_models`, `list_installed_stt_models` | `models-changed` |
| Inspect installed model | `useModelInspect` | `inspect_model` (the GGUF header) | — |
| Delete | `DownloadsInstalled` | `remove_model`, `delete_llama_model`, `delete_vllm_model`, `delete_stt_model` | `models-changed` |
| Storage | `Storage*` sections | `get_disk_usage`, `get_storage_path`, `validate_storage_path`, `resolve_models_folder`, `get/set_user_settings`, `clear_app_cache` | — |

The four progress events all funnel through one subscription (`downloadEventBus`) into one
Zustand store (`modelStore.downloads`). The installed list has its own single subscription
(`installedModelsBus` → `installedModelsStore`) on `models-changed`.

---

## Top-level utilities

These are pure, React-free modules — the math/parsing the tables depend on.

### `parse_quant.ts` — quant label from a filename
**Responsibility:** project a raw HF file path into the canonical quant token (`Q4_K_M`,
`IQ4_XS`, `BF16`, …). **Why:** the variant table and the install dispatch must agree on a
single label; matching is separator-bounded so `Q4_K_M` doesn't match inside an unrelated
substring. Longest-known-quant-first avoids `Q4_K` shadowing `Q4_K_M`. Returns `null` →
the table shows `"unknown"` and the model name is built without a `:quant` tag.

```ts
const SORTED = [...KNOWN_QUANTS].sort((a, b) => b.length - a.length);
const isSep = (ch) => ch === undefined || ch === "." || ch === "_" || ch === "-";
export function parseQuant(filename: string): string | null {
  const stem = filename.replace(/\.gguf$/i, "").toUpperCase();
  for (const q of SORTED) {
    const idx = stem.lastIndexOf(q);
    if (idx === -1) continue;
    if (isSep(stem[idx - 1]) && isSep(stem[idx + q.length])) return q; // bounded
  }
  return null;
}
```

### `fit.ts` — does-it-fit-in-memory
**Responsibility:** a `"fits" | "tight" | "wont-fit"` verdict + badge for a download size vs
available RAM. **Why:** the user judges *before* pulling GBs. Mirrors the Compare feature's
rule: a `1.3×` safety multiplier approximates runtime memory over on-disk size, and "tight"
kicks in above 70% of available memory. Pure — no React, no IPC. Consumed by `HfVariantTable`
(per-variant) and `VLlmRepoDetail` (summed snapshot size).

```ts
const SAFETY = 1.3, TIGHT_FRACTION = 0.7;
export function fitOfNeed(needBytes, availBytes): Fit {
  if (availBytes <= 0) return needBytes > 0 ? "wont-fit" : "fits";
  if (needBytes > availBytes) return "wont-fit";
  if (needBytes > availBytes * TIGHT_FRACTION) return "tight";
  return "fits";
}
export function memoryFit(sizeBytes, availBytes) {       // file-size-only path
  return fitOfNeed(Math.ceil(sizeBytes * SAFETY), availBytes);
}
```

`fitBadge(fit)` → `{ text, cls }` (green "Fits" / amber "Tight" / red "Won't fit").

### `classify_variant.ts` — refuse un-runnable GGUFs
**Responsibility:** classify an HF GGUF filename as `model | projection | adapter`. **Why:**
born from a real bug — a user installed `mmproj-…bf16.gguf`; the download succeeded, llama.cpp's
create stream closed without a success chunk, and `llama_cpp list` stayed empty. Better to never
let the click happen than detect after the fact. `mmproj*` → projection layer (multimodal,
not standalone); `*lora*`/`*adapter*` → adapter. `HfVariantTable` swaps the Install button for
a disabled "… · Not supported" cell with a hover `reason`.

### `format.ts` — `hfVariantModelName(filename, quant?)`
**Responsibility:** derive the llama.cpp model name from an HF GGUF filename. **Why:** llama.cpp
0.24+ rejects names with embedded dots that look like an untagged blob, so a quant is encoded
as `<base>:<quant>`. Uses only the *basename* (subdirs inflate past llama.cpp's length/pattern
limits → generic "invalid model name" 400), strips the trailing quant suffix so it doesn't
appear twice, and sanitizes illegal chars to `-`. Both the detail page (to mark installed) and
the install dispatch call this so they stay in sync.

```ts
export function hfVariantModelName(filename: string, quantization?: string): string {
  const stem = basename(filename).replace(/\.gguf$/i, "").toLowerCase();
  const base = sanitizeNameSegment(stem);
  if (!quantization) return base;
  const q = sanitizeNameSegment(quantization.toLowerCase());
  const stripped = base.replace(new RegExp(`[._-]${q}$`), "");
  return `${stripped}:${q}`;            // e.g. "llama-3-8b:q4_k_m"
}
```

---

## `state/` — stores, buses, install state

This folder is the spine: two singleton event buses, the install-progress store, the canonical
installed-model registry, and pure progress-derivation helpers.

### `modelStore.ts` — install/download UI store ⭐
**Responsibility:** the single Zustand store for the Models page — active sub-tab, the
`downloads` map keyed by model id, the per-source "active name" slots, pull-id → name map, and
HF search/repo selection. **Why:** progress events arrive from Rust with no React context; they
need a global sink. Every download tab subscribes here.

Shape of one in-flight/finished entry:

```ts
export interface DownloadEntry {
  id: string; source: DownloadSource; name: string;   // source: llama_cpp|huggingface|local
  status: "downloading" | "installing" | "success" | "error" | "cancelled";
  percent: number; bytesCompleted?: number; bytesTotal?: number;
  error?: string | null; pullId?: string; phaseLabel?: string;
}
upsertDownload: (e) => set((s) => ({ downloads: { ...s.downloads, [e.id]: e } })),
```

`activeHfName` (shared by GGUF + vLLM), `activeLocalName`, `activeSttName` route an event onto
the right entry; `pullNames[pullId]` maps a llama.cpp pull's id back to its display name.
`findActiveDownload()` picks the first in-flight entry for any "one summary line" surface.
`setHfRepoKind` drops the open repo detail (a GGUF detail makes no sense under vLLM).

### `downloadEventBus.ts` — one subscription for all four progress streams ⭐
**Responsibility:** idempotent singleton that registers one `listen()` per progress event and
fans them to typed handlers. **Why:** replaces N per-component listeners (one
listener-registration race per component). If the IIFE rejects (transient Tauri init failure),
the singleton resets so a later call retries instead of latching a rejected promise. Every
install hook fires `void startDownloadEventBus()` in a `useEffect`.

```ts
export function startDownloadEventBus(): Promise<void> {
  if (starting) return starting;
  starting = (async () => {
    await listen(EVENT_HF_PROGRESS,            (e) => onHf(e.payload));
    await listen(EVENT_PULL_PROGRESS,          (e) => onPull(e.payload));
    await listen(EVENT_LOCAL_INSTALL_PROGRESS, (e) => onLocal(e.payload));
    await listen(EVENT_STT_INSTALL_PROGRESS,   (e) => onStt(e.payload));
  })();
  starting.catch((e) => { console.error(...); starting = null; }); // retry on failure
  return starting;
}
```

### `downloadEventHandlers.ts` — parse → percent → upsert ⭐
**Responsibility:** for each raw event payload: zod-validate it, look up the active-name slot,
compute a percent, and `upsertDownload`. Invalid payloads are logged and dropped (never crash
the bus). **Why:** the bus stays dumb; all per-source phase logic lives here. Each source has
its own phase vocabulary, normalized to the common `status`/`phaseLabel`.

`onHf` handles the multi-phase HF install — download, then (when the active backend is llama.cpp)
hashing/uploading, then "Creating model":

```ts
export function onHf(payload: unknown) {
  const p = HfPhaseSchema.safeParse(payload);
  if (!p.success) { console.error("invalid hf-progress payload", p.error.issues); return; }
  const { activeHfName, upsertDownload } = useModelStore.getState();
  if (!activeHfName) return;                          // no install in flight → ignore
  const base = { id: activeHfName, source: "huggingface" as const, name: activeHfName };
  if (p.data.phase === "downloading") {
    const { bytes_completed: done, bytes_total: total } = p.data;
    upsertDownload({ ...base, status: "downloading", percent: pct(done, total),
      bytesCompleted: done, bytesTotal: total, phaseLabel: "Downloading" });
    return;
  }
  if (p.data.phase === "hashing" || p.data.phase === "uploading") { /* "installing" */ }
  upsertDownload({ ...base, status: "installing", percent: 100, phaseLabel: "Creating model" });
}
```

`onPull` resolves the name via `pullNames[pull_id]` (falling back to the event's `name`),
maps llama.cpp's `downloading|verifying|writing|success|failed` to `status`, and writes the
`pullId` so Cancel can target it. `onLocal` mirrors `onHf`'s hashing/uploading phases (no
network download — the file is already on disk). `onStt` is a thin downloading→success path.

### `installedModelsStore.ts` — the canonical installed-model registry ⭐
**Responsibility:** *single source of truth* for "what models are installed", consumed by every
model picker in the app (Workspace, Compare, Analysis, Tests). **Why:** one store, refreshed two
ways — proactively by install hooks on success, and centrally by `installedModelsBus` on the
backend's `models-changed` broadcast — so a new model appears even if the broadcast is dropped
(listener race, the weights folder lag).

```ts
refresh: async () => {
  if (get().status === "loading") return;            // de-dupe concurrent refreshes
  set({ status: "loading", error: null });
  const [llama_cpp, llama, vllm] = await Promise.allSettled([
    getInstalledModelsWithStats(), listLlamaModels(), listVLlmModels(),
  ]);
  const list: InstalledModelInfo[] = [];
  if (llama_cpp.status === "fulfilled") list.push(...llama_cpp.value);  // merge each backend
  if (llama.status  === "fulfilled") list.push(...llama.value);   // independently —
  if (vllm.status    === "fulfilled") list.push(...vllm.value);     // one down ≠ all down
  if (llama_cpp.status==="rejected" && llama.status==="rejected" && vllm.status==="rejected")
    return set({ status: "error", error: formatIpcError(llama_cpp.reason) });
  set({ list, status: "ready", error: null, lastRefreshedAt: Date.now() });
},
```

Each backend is fetched independently via `allSettled` — error is set only when *every* source
fails (vLLM legitimately yields `[]` off Apple Silicon).

### `installedModelsBus.ts` — one `models-changed` subscription
**Responsibility:** single shared subscription to the backend's `models-changed` event →
`installedModelsStore.refresh()`. Idempotent + self-resetting like `downloadEventBus`. Replaces
the per-component `listen()` calls each consumer used to register, killing the registration race
and duplicate fetches. Fires `refresh()` once on startup.

### `installedGroups.ts` — collapse merged list into one row per model
**Responsibility:** `groupInstalled(list)` → `ModelGroup[]`, one entry per model keyed on the
base name (stripping `:latest` so the llama.cpp tag and the llama.cpp folder stem collapse). **Why:**
a single model can exist in multiple backends; the Installed list shows one row with
llama.cpp / vLLM badges + the right actions instead of duplicate rows. Tracks `llama_cppName`
(for delete), `llamaPath` (for add-to-llama_cpp/delete), `vllmPath` + `displayName` (vLLM repo id).

### `install_state.ts` — llama.cpp pull progress derivation (pure)
**Responsibility:** the `ModelInstallState` shape + `deriveProgress` (bytes/speed → percent +
ETA, ETA capped at 99,999s) + `applyProgress` (map a `PullProgress` phase to a state). Pure,
unit-testable. Consumed by `useModelInstall` for the typed `state.progress`.

### `modelSettingsStore.ts` — per-model settings (temperature + thinking)
**Responsibility:** load-once map of `{ [model]: { temperature, is_thinking } }` from the backend
(YAML-backed), optimistic writes via `setModelTemperature` / `setModelThinking`. `temperatureFor(model)`
falls back to `DEFAULT_TEMPERATURE`; `isThinkingFor(model)` falls back to `false`. Each setter **merges
onto the existing entry** so writing one field never clobbers the other. `isThinkingFor(model)` returns
the explicit setting if present, else **auto-detects** from the name via `isLikelyThinkingModel`
(`shared/models/classify.ts`) — so a reasoning model (qwen3.x, QwQ, DeepSeek-R1, …) is pre-checked
without the user guessing. The "Thinking" checkbox per row in `ModelSelector.tsx` calls `setThinking`
(an explicit click persists and overrides the heuristic); `useBatchRun` stamps `is_thinking` onto each
`ModelTarget` at dispatch so the backend raises the token budget + strips `<think>` for reasoning
models. Loaded at app startup; consumed wherever a model is run.

---

## `hooks/` — install + inspect + browse

### `useHfInstall.ts` — Hugging Face GGUF install ⭐
**Responsibility:** drive an `install_hf_gguf` and expose a derived `state` synced from the
`downloads[activeHfName]` entry the bus is updating. **Why:** the hook *kicks off* the install;
the bus *reports* it — derived state keeps the button/progress UI in lockstep with live events.

Two non-obvious guards: (1) one-install-at-a-time — the backend cancels its prior token, so if
another HF/vLLM install is in flight the hook *refuses* rather than clobbering `activeHfName`
(which would route the old download's events onto the new entry); (2) self-heal — it calls
`installedModelsStore.refresh()` itself on success, not trusting the broadcast alone.

```ts
const install = useCallback(async (repo, filename, name) => {
  const store = useModelStore.getState();
  if (store.activeHfName && store.activeHfName !== name) {
    const cur = store.downloads[store.activeHfName];
    if (cur && (cur.status === "downloading" || cur.status === "installing")) {
      upsertDownload({ id: name, source: "huggingface", name, status: "error", percent: 0,
        error: `Another download is in progress (${store.activeHfName}). Cancel it first.` });
      return;                                          // refuse, don't clobber
    }
  }
  setActiveHfName(name);
  upsertDownload({ id: name, source: "huggingface", name, status: "downloading", percent: 0 });
  try {
    const backend = useBackendStore.getState().selectedBackend;   // llama.cpp imports / llama.cpp keeps GGUF
    await installHfGguf(repo, filename, name, backend);
    upsertDownload({ id: name, source: "huggingface", name, status: "success", percent: 100 });
    void useInstalledModelsStore.getState().refresh();             // self-heal
  } catch (e) {
    upsertDownload({ id: name, source: "huggingface", name, status: "error", percent: 0,
      error: friendlyInstallError(e) });
  }
}, [setActiveHfName, upsertDownload]);
```

`cancel` → `cancel_hf_install` (best-effort) then marks the entry `cancelled`.

### `useModelInstall.ts` — llama.cpp pull ⭐
**Responsibility:** `pull_model` → a `pullId` the bus needs to route `pull-progress`, and
`cancel_pull` targeting the in-flight pull. **Why:** llama.cpp's pull is async/streamed; the hook
records `pullNames[pullId] = name` so `onPull` can resolve the display name, and merges the
`pullId` into whatever entry exists by the time `invoke` returns (connection-refused fails fast
and the bus may already have written a Failed event). A `setTimeout(refresh, 1500)` backstops the
`models-changed` broadcast.

```ts
const pullId = await invoke<string>("pull_model", { name });
pullIdRef.current = pullId;
recordPullName(pullId, name);
const current = useModelStore.getState().downloads[name];
if (current) upsert({ ...current, pullId });          // merge, don't clobber a Failed event
setTimeout(() => { void useInstalledModelsStore.getState().refresh(); }, 1500);
```

### `useVLlmInstall.ts` — vLLM snapshot download ⭐
Mirrors `useHfInstall`: progress rides the shared bus keyed on the repo as `activeHfName`, and
shares the one-at-a-time in-flight slot with GGUF installs. Calls `install_vllm_model(repo)`
(snapshot-download into `~/.quantamind/vllm`), self-heals `refresh()`, cancels via
`cancel_hf_install`. No file-by-file percent — the entry flips downloading → success.

### `useLocalImport.ts` — local `.gguf` import ⭐
**Responsibility:** pick a file (dialog or drag-drop), inspect it, name it, place it in the shared weights folder.
**Why:** the only path that *reads metadata before* committing — `inspect_gguf` populates the
preview card so the user confirms family/params/ctx/quant. The default name is the filename stem
sanitized to llama.cpp-legal chars; a `conflict` flag warns if it already exists.

```ts
const choose = useCallback(async (p) => {
  setError(null); setPath(p); setName(defaultName(p));
  try { setMeta(await inspectGguf(p)); }                 // GGUF metadata → preview
  catch (e) { setError(formatIpcError(e)); setMeta(null); }
}, []);
const doImport = useCallback(async () => {
  if (!path) return;
  setActiveLocalName(name);
  try {
    await installLocalGguf(path, name);                  // hashing/uploading via local bus
    upsert({ id: name, source: "local", name, status: "success", percent: 100 });
    void useInstalledModelsStore.getState().refresh();   // self-heal
    cancel();
  } catch (e) { /* error entry + setError */ }
}, [...]);
```

`pendingLocalPath` (set by drag-drop) is auto-`choose`n via an effect. `busy`/`percent`/
`phaseLabel` are read off the live entry for the progress bar.

### `useHfRepoVariants.ts` — GGUF variant list ⭐
**Responsibility:** `hf_repo_files(repo)` → project each file into `{ filename, quantization, sizeBytes }`
via `parseQuant`. `refetch` bumps a nonce to re-run. A `cancelled` flag guards against
out-of-order resolves. This is the data behind `HfVariantTable`.

### `useModelInspect.ts` — installed-model metadata
`inspect_model(model, backend)` → the GGUF header data (chat template, capabilities, base-model
guess). Re-runs on model/backend change; `cancelled` guard. Backs `TemplatePanel`.

### `useHfModelCard.ts` — repo model card
`hf_model_card(repo)` → structured `ModelCard` (task/license/base/description/tags). `none` =
no README (not an error). Backs `ModelCardSection`/`ModelCardDetail` and `VLlmRepoDetail`'s
task guard.

| Hook | One line |
|---|---|
| `useHardwareSnapshot.ts` | Fetch hardware snapshot once for fit badges; `null` on failure → table omits the Fit column rather than guessing. |
| `useModalDragDrop.ts` | While active, listen for OS drag-drop; filter `.gguf`, set `pendingLocalPath` + switch to the Local tab. Multi/non-gguf drops log to devtools. |
| `useModelLabel.ts` | Resolve a bare wire id (vLLM path) → friendly label via the installed list + `shared/models/modelLabel`; falls back to the raw name. |

---

## `components/tabs/` — the three install sources + Downloads

### `HuggingFaceTab.tsx` — search & route ⭐
**Responsibility:** debounced (300ms) `hf_search(query, 30, kind)` with a GGUF/vLLM toggle, a
results grid, and routing into the right detail. **Why:** the toggle filters search, but a repo
opens by its *own* format, not the toggle — an `vllm`-tagged repo found under unfiltered GGUF
search still opens the vLLM action. A `seq` ref drops stale responses.

```ts
if (selected) {
  const isVLlm = selectedTags.some((t) => t.toLowerCase() === "vllm");
  return isVLlm ? <VLlmRepoDetail repo={selected} … /> : <HuggingFaceRepoDetail repo={selected} … />;
}
```

Search state (query/repo/kind) lives in `modelStore` so it survives tab switches. Each hit card
shows id, downloads/likes, and the first 4 tags.

### `llama.cppLibraryTab.tsx` — name-and-pull ⭐
**Responsibility:** free-text llama.cpp model name → `useModelInstall`. **Why:** llama.cpp's library
is huge and fast-moving; rather than mirror a catalog, the user types a name (or opens
`huggingface.co/models?library=gguf` externally) and pulls. Disables Install when the name is already installed
(checked against `installedModelsStore.list`); shows a `<progress>` while pulling with a Cancel
button.

### `LocalFileTab.tsx` — preview-then-import ⭐
**Responsibility:** thin view over `useLocalImport` — three states: (1) file chosen + metadata →
`LocalFilePreview`; (2) importing (no preview) → inline progress; (3) idle → drag-drop zone +
"Browse files…". The drag zone is wired by `useModalDragDrop`.

### `DownloadsTab.tsx` / `DownloadsActive.tsx` / `DownloadsInstalled.tsx`
- **`DownloadsTab`** — three stacked sections: Storage, In progress (`DownloadsActive`),
  Installed (`DownloadsInstalled`).
- **`DownloadsActive`** ⭐ — renders the visible (`downloading|installing|success|error`)
  entries of `modelStore.downloads` as `DownloadEntryRow`s. Auto-clears *success* entries after
  5s so completed installs don't accumulate; errors persist until dismissed. Cancel dispatches
  via `cancelEntry` then removes the entry + shows a "partial files cleaned" toast.
- **`DownloadsInstalled`** ⭐ — `groupInstalled(list)` rows with backend badges
  (llama.cpp / vLLM). Delete dispatches per-backend: `remove_model` (llama.cpp),
  `delete_llama_model` (file, opt-in via `ConfirmRemove` checkbox), and `delete_vllm_model`;
  then `refresh()`. llama.cpp-only rows offer `AddTllama_cpp.cppButton`.

### `cancelEntry.ts` — dispatch the right cancel
`huggingface` → `cancel_hf_install`; `llama_cpp` (+pullId) →
`cancel_pull`. Returns `null` on success or an `Error` (caller decides whether to keep the entry).

### `AddTllama_cpp.cppButton.tsx` — import a folder GGUF into llama.cpp ⭐
**Responsibility:** for a llama.cpp-only model, reuse `install_local_gguf(path, name)` so it's
runnable in llama.cpp too. Subscribes to `local-install-progress` for the duration, shows live
phase ("Hashing 42%" / "Uploading 80%" / "Creating…"), toasts the result, then `refresh()`.

| File | One line |
|---|---|
| `DownloadEntryRow.tsx` | Presentational row for one active download: name, source · status · bytes, progress bar (non-terminal), error text, Cancel (non-local)/Dismiss (terminal) button. |

---

## `components/` — HF/vLLM detail, variant table, install status

### `HuggingFaceRepoDetail.tsx` — GGUF repo install screen ⭐
**Responsibility:** wire `useHfRepoVariants` + `useHfInstall` + `useHardwareSnapshot` +
installed-set into a Back button, model card, variant table, and install status line. Builds
each variant's llama.cpp name via `hfVariantModelName` (so installed-detection matches) and dispatches
`install(repo, filename, name)`.

### `HfVariantTable.tsx` — variant table with size, fit, and guardrails ⭐
**Responsibility:** one row per GGUF variant with quant, HF download size (`formatBytes`),
fit badge (`memoryFit` vs snapshot), and an action that is one of: **Installed ✓** /
**blocked** (`classifyHfVariant` non-model → "… · Not supported", with a hover `reason`) /
**Install** button.

```tsx
const isInstalled = installed.has(nameOf(v));
const klass = classifyHfVariant(v.filename);
const blocked = klass.kind !== "model";              // mmproj / lora → can't install standalone
const fit = snapshot ? fitBadge(memoryFit(v.sizeBytes, snapshot.available_memory_bytes)) : null;
// … Installed ✓  |  "{label} · Not supported" (title=reason)  |  <Install/>
```

The Fit column only renders when a hardware snapshot loaded — no guessing.

### `VLlmRepoDetail.tsx` — vLLM snapshot install screen ⭐
**Responsibility:** model card, summed snapshot size (`hf_repo_all_files` → Σ sizes) + fit badge,
two guardrails, and a Download button → `useVLlmInstall`. **Why:** `vllm_lm.server` only serves
text-generation LLMs — anything else downloads gigabytes then can't answer a chat request.

- **Incompatible task** (card `pipeline_tag` known *and* ≠ `text-generation`) → an amber banner
  *and* a confirm dialog before download ("Download anyway" / "Pick another"). An *absent* tag is
  treated as "maybe" so untagged LLM repos aren't false-blocked.
- **Base model** heuristic (`…-pt`/`…-base` in the repo id) → a soft warning (not a block):
  won't follow chat prompts, prefer an `-it`/`Instruct` variant.

After download it points the user to the Workspace dropdown ("Start vLLM"; needs `pip install vllm-lm`).

### `HfInstallStatus.tsx` — shared install status line
Renders the `HfInstallState` (shared by HF + vLLM details): downloading (`<progress>` + Cancel),
installing ("Installing into llama.cpp…"), success ("Installed ✓ — open Workspace or Compare"),
error (message + dismiss).

### card/ — model card + template inspector
| File | One line |
|---|---|
| `card/ModelCardSection.tsx` | Collapsible "Show model card" that lazily fetches via `useHfModelCard` only when opened. |
| `card/ModelCardDetail.tsx` | Renders the card as a **structured data panel** — task/license/base badges, description, tags, "Open on HF" link. Never injects remote HTML (controlled values → native components). |
| `card/TemplatePanel.tsx` | the GGUF header inspector: base-model warning (`is_base_guess`), capability chips, and the chat template as inert `<pre>` text (never injected HTML). llama.cpp-only → "Not available" otherwise. |

| File | One line |
|---|---|
| `LocalFilePreview.tsx` | Preview card for a chosen `.gguf`: family/params/ctx/quant from metadata, name input (validated, conflict-warned), progress, import/cancel. `ImportError` turns parser errors (truncated / bad magic / unsupported version) into actionable plain-language guidance. |
| `ConfirmRemove.tsx` | Delete confirm; when a model is in both backends, an opt-in checkbox also deletes the llama.cpp GGUF (default off). |

---

## `components/` (page shells) + `components/storage/`

| File | One line |
|---|---|
| `ModelsPage.tsx` | Models top-view shell: three sub-tabs (llama.cpp/HF/Local) with Cmd+1–3 hotkeys, gated on `navStore.topView === "models"`. |
| `DownloadsPage.tsx` | Downloads top-view shell — just a header over `DownloadsTab`. |
| `storage/StorageSection.tsx` | Storage controls atop Downloads: storage-path + models-folder sections, disk-usage summary (`get_disk_usage`), and Clear-cache (`clear_app_cache`, which also resets the in-memory eval/batch/cliff stores the deleted caches backed). The confirm dialog has an opt-in "also clear HuggingFace model cache" checkbox → `clear_app_cache(includeModels=true)` wipes the re-downloadable vLLM snapshots under `~/.cache/huggingface` (HF auth token + the app's canonical `~/.quantamind` models are kept). |
| `StoragePathSection.tsx` | llama.cpp models path (`get_storage_path`) + validate a candidate dir (`validate_storage_path`: exists/dir/writable/≥50GB) + the `export QUANTAMIND_GGUF_DIR=…` snippet to make it permanent. |
| `ModelsFolderSection.tsx` | The shared GGUF weights folder (used by llama.cpp directly, imported into llama.cpp) via `resolve_models_folder` / `get/set_user_settings`; refreshes the installed list after a change. |
| `storage/ClearCacheConfirm.tsx` | Type-`CLEAR`-to-confirm guard for wiping regenerable caches; copy spells out that models/collections/settings are kept. |

---

## Data-flow walkthrough — browse → install → everywhere

1. **Browse.** User types in `HuggingFaceTab`; debounced `hf_search(q, 30, "gguf")` returns hits
   into a grid. Click a hit → `setHfSelectedRepo(id, tags)`; non-vLLM tags route to
   `HuggingFaceRepoDetail`.
2. **Pick a variant.** `useHfRepoVariants` runs `hf_repo_files(repo)`; each file → `parseQuant`
   for its label. `HfVariantTable` shows per-variant **size** (`formatBytes`) and **fit**
   (`memoryFit(sizeBytes, snapshot.available_memory_bytes)` → green/amber/red). `classifyHfVariant`
   disables `mmproj`/`lora` rows. Already-installed rows (matched via `hfVariantModelName`) show
   "Installed ✓".
3. **Fit check.** The fit badge is advisory (RAM): `memoryFit(sizeBytes, available_memory_bytes)`
   colours green/amber/red but never blocks the install.
4. **Install.** Click Install → `useHfInstall.install(repo, filename, name)`. Guard: refuse if
   another HF/vLLM install is in flight. `setActiveHfName(name)` + an initial `downloading` entry,
   then `install_hf_gguf(repo, filename, name, selectedBackend)`.
5. **Progress events.** Rust emits `hf-progress` → `downloadEventBus` → `onHf` validates, computes
   percent, `upsertDownload` → `modelStore.downloads[name]` updates → `HfInstallStatus` and
   `DownloadsActive` re-render live (Downloading → Hashing → Uploading → Creating model).
6. **Registry update.** On success `useHfInstall` calls `installedModelsStore.refresh()` directly,
   *and* Rust broadcasts `models-changed` → `installedModelsBus` → the same `refresh()`. The store
   re-fetches all backends (`get_installed_models_with_stats`, `list_llama_models`, `list_vllm_models`,
   `list_installed_stt_models`) and `setList`.
7. **Appears everywhere.** Because `installedModelsStore` is the single registry, the new model
   now shows in `DownloadsInstalled` (grouped, badged) *and* in every model picker — Workspace,
   Compare, Analysis, Tests — which all subscribe to the same store. No page reload, no per-picker
   re-fetch.

---

## See also

- [`backend-models-hf-gguf.md`](./backend-models-hf-gguf.md) — `hf_search`/`hf_repo_files`/
  `hf_model_card`/`install_hf_gguf`/`cancel_hf_install`, `inspect_gguf`/`install_local_gguf`,
  `pull_model`/`cancel_pull`, vLLM install/list/delete, the `hf-progress`/`pull-progress`/
  `local-install-progress` event contracts.
- [`backend-prompt-workspace-system.md`](./backend-prompt-workspace-system.md) — storage commands
  (`get_disk_usage`, `get/validate_storage_path`, `resolve_models_folder`, `clear_app_cache`).
- [`frontend-overview.md`](./frontend-overview.md) — shared IPC layer + `shared/models/modelLabel`.
- [`frontend-workspace.md`](./frontend-workspace.md), [`frontend-compare-analysis.md`](./frontend-compare-analysis.md)
  — model pickers that consume `installedModelsStore`.

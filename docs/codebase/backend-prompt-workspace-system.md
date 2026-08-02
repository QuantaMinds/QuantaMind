# Backend: Prompt, Workspace & System Support Subsystems

Documents the Rust IPC command layer that surrounds local-LLM generation:
the **single-prompt run** loop, **bundled prompt templates**, **workspaces +
run history**, **settings**, **storage accounting**, and **system/hardware**
introspection. These are the support subsystems — the generation engine itself
lives in [`backend-inference-backends.md`](backend-inference-backends.md), the
on-disk YAML/serde persistence in
[`backend-persistence.md`](backend-persistence.md), the Workspace UI in
[`frontend-workspace.md`](frontend-workspace.md), and the Settings UI in
[`frontend-support-features.md`](frontend-support-features.md).

## Overview

**Why.** Generation is one thin slice. Around it the app needs: a cancellable
single-turn runner that streams tokens to the UI; a library of starter prompts;
a way to organise prompts into projects on disk and remember what was run;
persistent per-model and global settings; honest disk-space accounting before
an install; and a hardware snapshot so every "will this model fit?" decision is
grounded in real RAM/VRAM, not a guess. Each lives in its own folder under
`backend/src/commands/`, each file owns one responsibility
([`docs/architecture.md#architecture`](../architecture.md)).

**What each folder does.**

| Folder | Responsibility |
|---|---|
| `prompt/` | `run_prompt` / `stop_prompt`: single-turn Workspace generation, streamed via the `InferenceBackend` trait, cancellable through `RunState`. |
| `prompt_templates/` | Read-only listing of the bundled `docs/prompts` markdown templates. |
| `workspace/` | Open/close/list a workspace folder; CRUD over its YAML prompt files; append/list/get/clear run history. |
| `settings/` | Model-storage path, per-model temperature, user settings (weights folders), models-folder resolution. |
| `storage/` | Installed-model disk stats, disk usage, regenerable-cache clear, a free-space guard. |
| `system/` | Hardware snapshot (GPU, RAM/VRAM, bandwidth), llama.cpp health/ps/RSS, onboarding scaffold. |

**How (IPC command surface).** Every public entry point is a `#[tauri::command]`
invoked from React via `invoke()`:

| Command | Folder · file | Kind |
|---|---|---|
| `run_prompt`, `stop_prompt` | prompt/prompt.rs | async / sync |
| `list_prompt_templates` | prompt_templates/templates.rs | sync |
| `open_workspace`, `close_workspace`, `current_workspace`, `list_workspace_tree`, `recent_workspaces` | workspace/workspaces.rs | sync |
| `load_prompt`, `save_prompt`, `create_prompt`, `rename_path`, `delete_path` | workspace/workspace_prompts.rs | sync |
| `history_append`, `history_list`, `history_get`, `history_clear`, `history_remove_by_path` | workspace/history.rs | sync |
| `get_storage_path`, `validate_storage_path` | settings/settings.rs | sync |
| `get_model_settings`, `set_model_temperature` | settings/model_settings.rs | sync |
| `get_user_settings`, `set_user_settings`, `resolve_models_folder` | settings/user_settings.rs | sync |
| `get_installed_models_with_stats`, `remove_model` | storage/storage.rs | async |
| `get_disk_usage` | storage/storage_usage.rs | async |
| `clear_app_cache` | storage/storage_cache.rs | sync |
| `get_hardware_snapshot` | system/hardware.rs | sync |
| `probe_gpu` (internal), via snapshot | system/gpu.rs | — |
| `get_loaded_models` | system/loaded_models.rs | async |
| `get_llama_cpp_rss` | system/process_memory.rs | sync |
| `check_llama_cpp_health` | system/health.rs | async |
| `scaffold_onboarding_workspace` | system/onboarding.rs | sync |

> Note: the IPC names in the task brief use shorthand (e.g. "open/close/current/
> list/recent workspace", "get/validate storage path"); the table above lists the
> exact registered command identifiers.

---

## `prompt/` — single-prompt run

The single-turn Workspace generation path. `run_prompt` resolves a backend +
endpoint, registers a cancellation token, streams every token to the UI over a
Tauri event, and emits a final `prompt-done` (or `prompt-cancelled`) payload
carrying timing. The brief notes there is a `mod.rs` listing `prompt`,
`prompt_options`, `prompt_payloads`, `prompt_run`.

### prompt/prompt.rs

- **File:** `backend/src/commands/prompt/prompt.rs`
- **Responsibility:** The `run_prompt` / `stop_prompt` commands and the
  `RunState` cancellation registry.
- **Why:** One run at a time, instantly stoppable; a new run must pre-empt the
  previous one rather than racing it. Streaming tokens must reach the UI live.
- **What:** `RunState { current: Mutex<Option<CancellationToken>> }`; the three
  event names (`prompt-token`, `prompt-done`, `prompt-cancelled`); the async
  `run_prompt` orchestration; the sync `stop_prompt` that cancels the live token.
- **How/Where used:** Frontend Workspace "Run" button → `run_prompt`; "Stop" →
  `stop_prompt`. Token events feed the streaming output pane.

```rust
#[tauri::command]
pub async fn run_prompt(
    app: tauri::AppHandle, state: tauri::State<'_, RunState>,
    settings: tauri::State<'_, ModelSettingsState>,
    model: String, prompt: String, system: Option<String>,
    params: Option<InferenceParams>, backend: Option<BackendKind>, keep_alive: Option<i32>,
) -> Result<(), AppError> {
    let backend = backend.unwrap_or_default();
    settings.ensure_loaded(&app)?;
    if let Some(p) = &params { validate_params(p)?; }
    let mut options = params.as_ref().map(to_generate_options).unwrap_or_default();
    if options.temperature.is_none() {              // fall back to per-model temp
        options.temperature = Some(settings.temperature_for(&model));
    }
    let token = CancellationToken::new();
    {   let mut guard = state.current.lock_recover();
        if let Some(prev) = guard.take() { prev.cancel(); }   // pre-empt prior run
        *guard = Some(token.clone()); }
    let timing = Arc::new(Mutex::new(RunTiming::start()));
    let emit_app = app.clone();
    let handler = make_token_handler(
        move |t| emit_app.emit(EVENT_TOKEN, TokenPayload { text: t.to_string() }).map_err(|_| ()),
        token.clone(), timing.clone());
    // vLLM uses the app-managed server's dynamic port; others use their default.
    let vllm_ep = vllm_endpoint();
    let ep = if backend == BackendKind::VLlm { vllm_ep.as_str() } else { endpoint::default_for(backend) };
    let result = run_prompt_inner(backend, ep, &model, &prompt,
        system.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        Some(options), keep_alive, token.clone(), handler).await;
    *state.current.lock_recover() = None;
    if let Ok(stats) = &result {
        if token.is_cancelled() {
            let count = timing.lock_recover().token_count;
            app.emit(EVENT_CANCELLED, CancelledPayload { token_count: count })...?;
        } else { app.emit(EVENT_DONE, &done_payload(&timing, stats))...?; }
    }
    result.map(|_| ())
}

#[tauri::command]
pub fn stop_prompt(state: tauri::State<'_, RunState>) -> Result<(), AppError> {
    if let Some(token) = state.current.lock_recover().take() { token.cancel(); }
    Ok(())
}
```

Key behaviours: backend defaults via `BackendKind::default()`; temperature
falls back to the per-model setting when the prompt left it unset; only vLLM uses
the dynamic app-managed port. The token handler closes over the cancel token and
`RunTiming`, so cancellation stops emission and timing records TTFT / tok/s.

### prompt/prompt_options.rs

- **File:** `backend/src/commands/prompt/prompt_options.rs`
- **Responsibility:** Validate `InferenceParams` ranges and map them to llama.cpp's
  `GenerateOptions` block.
- **Why:** Reject nonsensical sampling params at the boundary; translate the
  persisted schema's field names to the wire names (`max_tokens` → `num_predict`).
- **What:** `validate_params` (range guards), `to_generate_options` (field map).
- **How/Where used:** Called by `run_prompt` before dispatch. Has a sibling
  `prompt_options_tests.rs`.

```rust
pub fn validate_params(p: &InferenceParams) -> AppResult<()> {
    in_range("temperature", p.temperature, 0.0, 2.0)?;
    in_range("top_p", p.top_p, 0.0, 1.0)?;
    in_range("repeat_penalty", p.repeat_penalty, 0.0, 2.0)?;
    if let Some(n) = p.num_ctx { if n < 1 {
        return Err(AppError::Validation("num_ctx must be at least 1".into())); } }
    Ok(())
}
pub fn to_generate_options(p: &InferenceParams) -> GenerateOptions {
    GenerateOptions { temperature: p.temperature, top_p: p.top_p, top_k: p.top_k,
        num_predict: p.max_tokens, repeat_penalty: p.repeat_penalty,
        seed: p.seed, num_ctx: p.num_ctx }
}
```

### prompt/prompt_payloads.rs

- **File:** `backend/src/commands/prompt/prompt_payloads.rs`
- **Responsibility:** The serde event payload structs (`TokenPayload`,
  `DonePayload`, `CancelledPayload`) and `done_payload` assembly.
- **Why:** A run's outcome must reach the UI with real timing — never a
  fabricated zero indistinguishable from a real empty run
  ([`docs/architecture.md#robustness`](../architecture.md)).
- **What:** `DonePayload` carries `ttft_ms`, `tokens_per_sec`, `token_count`, a
  per-token `timeline`, and `GenerateStats`. `done_payload` reads the timing
  mutex with panic recovery.
- **How/Where used:** Emitted by `run_prompt`. Tests assert the timeline length
  equals the token count and that an empty run yields an empty timeline.

### prompt/prompt_run.rs

- **File:** `backend/src/commands/prompt/prompt_run.rs`
- **Responsibility:** Backend-agnostic dispatch — `run_prompt_inner` builds a
  `GenerateSpec` and calls `.generate()` on the chosen `InferenceBackend`.
- **Why:** Keep the command (`prompt.rs`) free of backend selection; one place
  fans out to llama.cpp / vLLM.
- **What:** `validate(model, prompt)` (non-empty), then a `match` on
  `BackendKind` constructing `LlamaCppBackend` or `VLlmBackend`.
- **How/Where used:** Called by `run_prompt`; re-exported as `run_prompt_inner`.
  See [`backend-inference-backends.md`](backend-inference-backends.md) for the
  trait and each backend.

```rust
pub async fn run_prompt_inner(backend: BackendKind, endpoint: &str, model: &str,
    prompt: &str, system: Option<&str>, options: Option<GenerateOptions>,
    keep_alive: Option<i32>, cancel: CancellationToken, on_token: impl FnMut(&str),
) -> AppResult<GenerateStats> {
    validate(model, prompt)?;
    let spec = GenerateSpec { model: model.into(), prompt: prompt.into(),
        system: system.map(str::to_string), options, keep_alive };
    match backend {
        BackendKind::LlamaCpp => LlamaCppBackend::new(endpoint.into()).generate(&spec, cancel, on_token).await,
        BackendKind::VLlm     => VLlmBackend::new(endpoint.into(), api_key, model.into()).generate(&spec, cancel, on_token).await,
    }
}
```

---

## `prompt_templates/` — bundled prompt templates

### prompt_templates/templates.rs

- **File:** `backend/src/commands/prompt_templates/templates.rs`
- **Responsibility:** List the bundled `*.md` prompt templates as
  `{ name, body }`.
- **Why:** Ship a starter library of prompts the UI can drop into a new prompt.
  `docs/prompts/` is a runtime app asset, not engineering docs (per `CLAUDE.md`).
- **What:** `PromptTemplate { name, body }`; `read_templates(dir)` reads every
  `*.md` (stem = name, file = body), sorted; `templates_dir` resolves env
  override → packaged `resources/prompts` → dev source tree. Missing dir → `[]`,
  never an error.
- **How/Where used:** Settings/Workspace UI calls `list_prompt_templates` to
  populate a template picker. Sibling `templates_tests.rs`.

```rust
pub fn read_templates(dir: &Path) -> AppResult<Vec<PromptTemplate>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| AppError::Io(e.to_string()))?.flatten() {
        let p = e.path();
        if let Some(stem) = p.file_name().and_then(|s| s.to_str()).and_then(|n| n.strip_suffix(".md")) {
            let body = std::fs::read_to_string(&p).map_err(|e| AppError::Io(e.to_string()))?;
            out.push(PromptTemplate { name: stem.to_string(), body });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
// resolution: QUANTAMIND_PROMPTS_DIR → resource_dir()/prompts → (debug) ../docs/prompts
```

---

## `workspace/` — workspaces, prompt CRUD & history

A workspace is a directory of YAML prompt files. `WorkspaceState` holds the
currently-open root (a `Mutex<Option<PathBuf>>`); every file operation is path-
confined to that root via `ensure_within`/`resolve_new` (rejects `..` / symlink
escapes). Run history lives in a hidden `.quantamind/` subdir.

### workspace/workspaces.rs

- **File:** `backend/src/commands/workspace/workspaces.rs`
- **Responsibility:** Open/close/inspect the active workspace; maintain the
  recent-workspaces list; expose the path-confinement helpers.
- **Why:** A single source of truth for "which folder are we editing" plus a
  containment guard so no command can touch files outside it.
- **What:** `WorkspaceState` with `root()`, `ensure_within()`, `resolve_new()`;
  commands `open_workspace` (canonicalise, set root, record recent, return file
  tree), `close_workspace`, `current_workspace`, `list_workspace_tree`,
  `recent_workspaces`. Recents persist to `recent_workspaces.yaml` under
  `app_config_dir`.
- **How/Where used:** Workspace UI opens a folder, lists its tree, shows recents.
  All prompt/history commands read `state.root()`. Sibling `workspaces_tests.rs`.

```rust
pub fn ensure_within(&self, candidate: &Path) -> AppResult<PathBuf> {
    p_io::ensure_within(&self.root()?, candidate)   // rejects ../ and symlink escapes
}

#[tauri::command]
pub fn open_workspace(app, state, path: String) -> Result<Vec<TreeNode>, AppError> {
    let root = PathBuf::from(&path);
    if !root.is_dir() { return Err(AppError::Validation(format!("not a directory: {path}"))); }
    let abs = root.canonicalize()...?;
    state.set(abs.clone());
    let mut list = load_recents(&recents_path(&app)?)?;
    record_recent(&mut list, RecentEntry { path: abs.display().to_string(), opened_at: now_utc() });
    save_recents(&recents_path(&app)?, &list)?;
    tree::list(&abs)
}
```

### workspace/workspace_prompts.rs

- **File:** `backend/src/commands/workspace/workspace_prompts.rs`
- **Responsibility:** CRUD over `*.quantamind.yaml` prompt files inside the
  workspace.
- **Why:** Prompts are user-authored YAML; reads/writes must round-trip exactly
  and stay confined to the workspace root.
- **What:** `load_prompt` (read), `save_prompt` (stamps `updated_at`, writes),
  `create_prompt` (validates name — non-empty, no path separators — and refuses
  to overwrite), `rename_path` (uses `resolve_new`, refuses to clobber),
  `delete_path` (deletes, returns refreshed tree). All paths flow through
  `validated()` → `ensure_within`.
- **How/Where used:** Workspace editor: open a prompt → `load_prompt`; save →
  `save_prompt`; "New" → `create_prompt`. Serde round-trip in
  [`backend-persistence.md`](backend-persistence.md).

```rust
#[tauri::command]
pub fn save_prompt(state, path: String, file: PromptFile) -> Result<PromptFile, AppError> {
    let target = validated(&state, &path)?;   // ensure_within
    let mut updated = file;
    updated.updated_at = now_utc();
    p_io::write(&target, &updated)?;          // serde_yaml round-trip
    Ok(updated)
}
```

The round-trip itself (`persistence/prompts/io.rs`) is plain `serde_yaml`, and
`PromptFile` is read-tolerant of the legacy `params` block (read, never written
back — global params are now the single source):

```rust
// persistence/prompts/io.rs
pub fn read(path: &Path) -> AppResult<PromptFile> {
    ...; serde_yaml::from_str(&content).map_err(|e| AppError::Internal(e.to_string()))
}
pub fn write(path: &Path, file: &PromptFile) -> AppResult<()> {
    let yaml = serde_yaml::to_string(file)...?; std::fs::write(path, yaml)...
}
```

### workspace/history.rs

- **File:** `backend/src/commands/workspace/history.rs`
- **Responsibility:** Append/list/get/clear/remove run-history records in
  `.quantamind/history.yaml`, with full outputs spilled to `.quantamind/runs/`.
- **Why:** Keep the history index small (previews + metrics) while preserving the
  full text per run; bounded so old runs evict and their blobs are pruned.
- **What:** `AppendArgs` (model/system/user/params/output + metrics); `qdir`,
  `runs_dir`, `history_path`; `history_append` (writes `<uuid>.txt`, records an
  index entry with a preview + char-count + metrics, evicts overflow and deletes
  evicted blobs), `history_list`, `history_get` (reads the blob), `history_clear`
  (resets index, removes `runs/`), `history_remove_by_path`.
- **How/Where used:** After a run completes the frontend calls `history_append`;
  the History panel lists entries and fetches full output on click. Index logic
  (`record`, `preview`, eviction) in
  [`backend-persistence.md`](backend-persistence.md).

```rust
#[tauri::command]
pub fn history_append(state, entry: AppendArgs) -> Result<(), AppError> {
    let q = qdir(&state)?; let runs = runs_dir(&q);
    std::fs::create_dir_all(&runs)...?;
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::write(runs.join(format!("{id}.txt")), &entry.output)...?;   // full output blob
    let mut h = history::load(&history_path(&q))?;
    let rec = HistoryEntry { id, ..., output_preview: history::preview(&entry.output),
        output_len: entry.output.chars().count(), token_count: entry.token_count,
        ttft_ms: entry.ttft_ms, tokens_per_sec: entry.tokens_per_sec, ran_at: now_utc() };
    let evicted = history::record(&mut h, rec);
    history::save(&history_path(&q), &h)?;
    for e in evicted { let _ = std::fs::remove_file(runs.join(format!("{}.txt", e.id))); }
    Ok(())
}
```

---

## `settings/` — model settings, user settings, storage path

Three persisted concerns, each a lazily-loaded `Mutex` state behind YAML in
`app_config_dir`.

### settings/model_settings.rs

- **File:** `backend/src/commands/settings/model_settings.rs`
- **Responsibility:** Per-model temperature, persisted to `model_settings.yaml`.
- **Why:** Different models want different default temperatures; `run_prompt`
  falls back to this when a prompt sets none. Default is `0.7`.
- **What:** `ModelSettingsState { inner, loaded }`; `ensure_loaded`,
  `temperature_for(model)` (→ `DEFAULT_TEMPERATURE` if unset),
  `validate_temperature` (0.0–2.0), commands `get_model_settings`,
  `set_model_temperature`.
- **How/Where used:** Settings UI sets a model's temperature; `run_prompt`
  reads `temperature_for`. Sibling `model_settings_tests.rs`.

```rust
pub const DEFAULT_TEMPERATURE: f32 = 0.7;
pub fn temperature_for(&self, model: &str) -> f32 {
    self.inner.lock_recover().get(model).map(|s| s.temperature).unwrap_or(DEFAULT_TEMPERATURE)
}
#[tauri::command]
pub fn set_model_temperature(app, state, model: String, temperature: f32) -> Result<(), AppError> {
    let trimmed = model.trim();
    if trimmed.is_empty() { return Err(AppError::Validation("model is empty".into())); }
    validate_temperature(temperature)?;                 // 0.0..=2.0, finite
    state.ensure_loaded(&app)?;
    let snapshot = { let mut g = state.inner.lock_recover();
        g.insert(trimmed.to_string(), ModelSettings { temperature }); g.clone() };
    save_map(&settings_path(&app)?, &snapshot)          // → model_settings.yaml
}
```

### settings/user_settings.rs

- **File:** `backend/src/commands/settings/user_settings.rs`
- **Responsibility:** User settings (the shared weights-folder override and the
  remote vLLM endpoints) in `user_settings.yaml`;
  resolve the effective GGUF folder.
- **Why:** Let the user relocate model storage; provide one resolution point
  combining user setting → env → default. The remote backends' URL + bearer key
  live here because they run off-box (no static localhost default).
- **What:** `UserSettingsState`; `weights_dir` (user → `QUANTAMIND_GGUF_DIR` →
  `~/.quantamind/gguf`), `vllm_weights_dir`, `stt_engine_dir`; the remote fields
  `vllm_url`/`vllm_api_key`/`vllm_url`/`vllm_api_key`; commands
  `get_user_settings`, `set_user_settings`, `resolve_models_folder` (absolute
  GGUF folder for display).
- **How/Where used:** Settings UI reads/writes user settings and shows the
  resolved models folder; downloaders consult `weights_dir`/`vllm_weights_dir`.
  On load and on every save, `push_remote_endpoints` mirrors the vLLM
  URL+key into `inference/backend/remote_config` so the Tauri-free dispatch path
  (`endpoint::resolve`) can read them.

### settings/settings.rs

- **File:** `backend/src/commands/settings/settings.rs`
- **Responsibility:** Report the llama.cpp models-storage path and validate a
  candidate relocation directory.
- **Why:** Before pointing llama.cpp at a new disk the app must confirm it exists,
  is a writable+renameable dir, and has enough free space (HF resume renames
  `*.partial`).
- **What:** `StoragePathInfo { current_path, from_env }`; `PathValidation`
  (exists / is_dir / writable / free+total bytes / `sufficient` ≥ 50 GB);
  `test_writable` (probes write **and** rename); commands `get_storage_path`,
  `validate_storage_path`.
- **How/Where used:** Settings → storage location panel. Reuses
  `compute_disk_usage` / `models_dir` from `storage/`.

```rust
const MIN_FREE_BYTES: u64 = 50 * 1024 * 1024 * 1024;   // ≥50 GB to relocate model storage
#[tauri::command]
pub fn validate_storage_path(path: String) -> Result<PathValidation, AppError> {
    let p = PathBuf::from(&path);
    let exists = p.exists(); let is_dir = exists && p.is_dir();
    let writable = is_dir && test_writable(&p);          // write + rename probe
    let usage = compute_disk_usage(&p, 0);
    Ok(PathValidation { exists, is_dir, writable, free_bytes: usage.free_bytes,
        total_bytes: usage.total_bytes, sufficient: usage.free_bytes >= MIN_FREE_BYTES })
}
```

---

## `storage/` — installed-model stats, disk usage, cache clear

### storage/storage.rs

- **File:** `backend/src/commands/storage/storage.rs`
- **Responsibility:** List installed llama.cpp models with size/family/quant stats;
  delete a model.
- **Why:** The model picker and storage panel need real per-model footprints;
  deletion must invalidate the UI's model list.
- **What:** `fetch_installed_with_stats` (GET the weights folder, map to
  `InstalledModelInfo`, sort by size desc), `remove_model_inner` (DELETE
  `/api/delete`, 404 → `NotFound`); commands `get_installed_models_with_stats`,
  `remove_model` (emits `EVENT_MODELS_CHANGED` on success).
- **How/Where used:** Storage/Models UI. `disk_usage_for` reuses
  `fetch_installed_with_stats` to sum model bytes.

### storage/storage_types.rs

- **File:** `backend/src/commands/storage/storage_types.rs`
- **Responsibility:** Serde DTOs for the storage layer.
- **Why:** Shared shapes between fetch, usage, and the the weights folder wire format.
- **What:** `InstalledModelInfo` (name, size, family, parameter_size, quant,
  `backend`, optional `digest`/`display_name`/`path`), `DiskUsage`
  (`total_bytes`, `free_bytes`, `llama_cpp_models_bytes`), and the internal
  `TagsResponse` / `ModelEntry` / `ModelDetails` deserialize structs.
- **How/Where used:** Returned by `get_installed_models_with_stats` and
  `get_disk_usage`; `digest` lets the picker collapse one blob shared across tags.

### storage/storage_disk.rs

- **File:** `backend/src/commands/storage/storage_disk.rs`
- **Responsibility:** Resolve every on-disk model folder and compute per-disk
  total/free bytes.
- **Why:** One place that knows where llama.cpp / GGUF / vLLM weights live and how to
  size the disk holding them; UI must never show a relative path.
- **What:** `absolutize`; `models_dir` (`QUANTAMIND_GGUF_DIR` → `~/.llama_cpp/models`);
  `gguf_dir_resolved` (setting → `QUANTAMIND_GGUF_DIR` → `~/.quantamind/gguf`)
  and `gguf_dest`; `vllm_dir_resolved` + `vllm_model_dir`; `compute_disk_usage`
  (picks the longest-matching mount, falls back to zeros).
- **How/Where used:** Used by `settings.rs`, `user_settings.rs`,
  `storage_usage.rs`, and the downloaders. Sibling `storage_disk_tests.rs`.

```rust
pub fn compute_disk_usage(probe_path: &Path, models_bytes: u64) -> DiskUsage {
    let disks = Disks::new_with_refreshed_list();
    let best = disks.list().iter()
        .filter(|d| probe_path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());   // longest mount wins
    let (total, free) = match best {
        Some(d) => (d.total_space(), d.available_space()),
        None => (0u64, 0u64),                                  // exotic mount → zeros, no panic
    };
    DiskUsage { total_bytes: total, free_bytes: free, llama_cpp_models_bytes: models_bytes }
}
```

### storage/storage_usage.rs

- **File:** `backend/src/commands/storage/storage_usage.rs`
- **Responsibility:** Combine model-byte sum with disk free/total into one
  `DiskUsage`.
- **Why:** Storage info must survive a down llama.cpp — only the model-bytes sum
  zeroes, never the whole panel.
- **What:** `disk_usage_for(endpoint)` (sum the weights folder sizes, default 0 on
  failure, then `compute_disk_usage(models_dir())`); command `get_disk_usage`.
- **How/Where used:** Storage panel. Sibling `storage_usage_tests.rs`.

> **2 GB / free-space guards.** The brief calls out a "refuses installs under
> 2 GB free" guard. It is **not** in this folder — the storage layer here
> reports usage and enforces a **50 GB** relocation floor (`settings.rs`). The
> per-install 2 GB pre-flight guard lives in the download path
> ([`backend-inference-backends.md`](backend-inference-backends.md)); it
> consumes `get_disk_usage` / `compute_disk_usage` from here. This doc documents
> the accounting; the install gate consumes it.

### storage/storage_cache.rs

- **File:** `backend/src/commands/storage/storage_cache.rs`
- **Responsibility:** Wipe regenerable caches under `app_config_dir`; report
  bytes freed.
- **Why:** Reclaim space without ever destroying user-authored data or settings.
- **What:** Allow-lists `CACHE_DIRS` (`jobs`, `history`, `batch_reports`,
  `traces`, `cliff`) and `CACHE_FILES` (`recent_workspaces.yaml`); `dir_size`
  (recursive sum), `clear_cache_in` (sum then delete; missing = clean skip);
  command `clear_app_cache`. Models, eval collections, readiness profiles, and
  `*_settings.yaml` are deliberately excluded.
- **How/Where used:** Settings → "Clear cache". Sibling `storage_cache_tests.rs`.

---

## `system/` — hardware, health, loaded models, onboarding

Gathers the hardware/runtime snapshot consumed everywhere a memory-fit
decision is made (the "will this model fit?" VRAM/RAM math).

### system/hardware.rs

- **File:** `backend/src/commands/system/hardware.rs`
- **Responsibility:** Build the `HardwareSnapshot` (RAM, CPU, cores, OS, arch,
  GPU, estimated bandwidth).
- **Why:** The single grounding fact for every "will this model fit?" check; one
  refreshed `sysinfo::System` behind a `OnceLock<Mutex<…>>`.
- **What:** `HardwareSnapshot` struct; `snapshot()` (refresh memory, compute
  available, read CPU brand, guess bandwidth, probe GPU); command
  `get_hardware_snapshot`. Inline tests assert total > 0 and available ≤ total.
- **How/Where used:** Consumed by the compare memory-fit gate, model picker
  fit badges, and the hardware panel.

```rust
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HardwareSnapshot {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub is_apple_silicon: bool,
    pub cpu: String,
    pub physical_cores: Option<usize>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub arch: String,
    pub gpu: GpuInfo,
    pub estimated_bandwidth_gbps: Option<u32>,   // None ⇒ "Not available", never fabricated
}
```

### system/gpu.rs

- **File:** `backend/src/commands/system/gpu.rs`
- **Responsibility:** Best-effort GPU/VRAM probe.
- **Why:** Cross-platform VRAM facts without fabrication; Apple Silicon has no
  separate VRAM pool (unified memory).
- **What:** `GpuInfo { name, vram_total/free_bytes, unified, available,
  gpu_working_set_bytes }`; `parse_nvidia_csv` (pure), `nvidia()` (runs `nvidia-smi`),
  `apple()` (reads the chip brand, marks `unified`, and via `macos_working_set_bytes()`
  reads the GPU's Metal `recommendedMaxWorkingSetSize` — the measured unified-memory cap
  the GPU can wire down, ~66-75% of RAM; `None` off macOS), `probe_gpu()` (NVIDIA → Apple
  → unavailable). `gpu_working_set_bytes` is what the context-ceiling meters budget against
  on Apple Silicon (see `llama_runtime::usable_memory_bytes`), not raw RAM.
- **How/Where used:** Embedded in `HardwareSnapshot.gpu`. Inline tests cover CSV
  parsing and "never panics".

### system/hardware_mem.rs

- **File:** `backend/src/commands/system/hardware_mem.rs`
- **Responsibility:** Memory-availability fallback and Apple-Silicon bandwidth
  lookup.
- **Why:** sysinfo sometimes returns 0 available on macOS; LLM token speed is
  bound by unified-memory bandwidth, not FLOPS — but only publish known figures.
- **What:** `compute_available` (use `available` if > 0, else `total − used`);
  `guess_memory_bandwidth_gbps` (M1–M4 Pro/Max/Ultra ordered before bare gen;
  unknown → `None`). Thorough inline tests including "Pro not shadowed by base"
  and "unknown is None not fabricated".
- **How/Where used:** Called by `hardware.rs::snapshot`.

### system/health.rs

- **File:** `backend/src/commands/system/health.rs`
- **Responsibility:** Probe llama.cpp liveness + version.
- **Why:** Gate UI on whether the runtime is up; a 2500 ms budget tolerates a
  busy server loading a large model while still failing fast on a real outage.
- **What:** `HealthStatus { available, version }`; `probe_health(endpoint)`. The
  per-backend command wrappers live with their backends (`check_llama_health`,
  `check_vllm_health`, `check_vllm_health`).
- **How/Where used:** App-wide llama.cpp status indicator / readiness gates.

### system/loaded_models.rs

- **File:** `backend/src/commands/system/loaded_models.rs`
- **Responsibility:** List the currently-resident model (the running llama-server's).
- **Why:** Show what's resident and how much sits in VRAM vs RAM; degrade to
  empty (not error) when llama.cpp is unreachable.
- **What:** `LoadedModel { name, size_bytes, size_vram_bytes, context_length }`;
  `fetch_loaded` (empty on any failure); command `get_loaded_models`.
- **How/Where used:** Latency tab / live-memory view, leak heuristics.

### system/process_memory.rs

- **File:** `backend/src/commands/system/process_memory.rs`
- **Responsibility:** Total RSS of all `llama_cpp` processes.
- **Why:** Feed the frontend's basic memory-leak heuristic (server + runner).
- **What:** `llama_cpp_rss()` (sum `.memory()` of processes whose name contains
  "llama_cpp"; `None` if zero); command `get_llama_cpp_rss`. Inline "never panics" test.
- **How/Where used:** Sampled per run by the leak heuristic.

### system/onboarding.rs

- **File:** `backend/src/commands/system/onboarding.rs`
- **Responsibility:** Scaffold a starter workspace with a welcome prompt.
- **Why:** First-run users get a ready-to-run prompt that shows off streaming;
  must be idempotent (never clobber an edited welcome).
- **What:** `welcome_prompt(now)` (a friendly two-line-poem prompt), `scaffold_in`
  (create `~/Documents/QuantaMind`, write `welcome.quantamind.yaml` only if
  absent); command `scaffold_onboarding_workspace`. Sibling `onboarding_tests.rs`.
- **How/Where used:** Onboarding flow → then `open_workspace` on the returned
  path.

```rust
pub fn scaffold_in(root: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(root)...?;
    let welcome = root.join("welcome.quantamind.yaml");
    if !welcome.exists() { io::write(&welcome, &welcome_prompt(now_utc()))?; }  // idempotent
    Ok(welcome)
}
```

---

## Data-flow walkthroughs

### (a) Workspace single-prompt run

1. UI calls `run_prompt(model, prompt, system?, params?, backend?, keep_alive?)`.
2. `prompt.rs` `ensure_loaded`s model settings, `validate_params`, maps to
   `GenerateOptions`, fills temperature from `temperature_for(model)` if unset.
3. A fresh `CancellationToken` is stored in `RunState`, cancelling any prior run.
4. Endpoint resolved (vLLM → dynamic app port; others → backend default).
5. `run_prompt_inner` builds a `GenerateSpec` and dispatches to the matching
   `InferenceBackend.generate()`; the token handler emits `prompt-token` per
   chunk and records `RunTiming` (TTFT, tok/s).
6. On finish: `prompt-cancelled { token_count }` if stopped, else
   `prompt-done` with the full `DonePayload` (timing + timeline + `GenerateStats`).
7. UI then calls `history_append`, spilling the full output to
   `.quantamind/runs/<uuid>.txt` and recording a preview + metrics in
   `history.yaml`. `stop_prompt` cancels mid-stream at any point.

### (b) Hardware snapshot feeding memory-fit / fit

1. UI (model picker, compare panel) calls `get_hardware_snapshot`.
2. `hardware.rs::snapshot` refreshes a shared `sysinfo::System`, computes
   available RAM via `compute_available`, reads the CPU brand, looks up
   `guess_memory_bandwidth_gbps`, and embeds `probe_gpu()`.
3. The snapshot (RAM/VRAM, unified flag, bandwidth) is what the memory-fit
   logic compares model footprints against. `get_loaded_models` and
   `get_llama_cpp_rss` supplement it with live residency for leak/over-commit
   checks. Anything unknown is `None` → "Not available", never fabricated.

### (c) Workspace open + prompt save round-trip

1. UI calls `open_workspace(path)`; `workspaces.rs` canonicalises it, sets
   `WorkspaceState.root`, records it in `recent_workspaces.yaml`, returns the
   file tree.
2. UI opens a prompt → `load_prompt(path)`; `workspace_prompts.rs` runs the path
   through `ensure_within` then `serde_yaml::from_str` → `PromptFile`.
3. User edits and saves → `save_prompt(path, file)`; `updated_at` is stamped and
   `serde_yaml::to_string` writes it back. Because the only mutation is
   `updated_at`, an unedited load→save is byte-identical except the timestamp —
   the round-trip is exact (legacy `params` is read-tolerant, never re-written).
4. `create_prompt` / `rename_path` / `delete_path` perform the same confinement
   check; deletion returns the refreshed tree for the UI to re-render.

---

## Cross-links

- **Generation engine** (`InferenceBackend`, `GenerateSpec`, llama.cpp / vLLM
  backends, the download/install 2 GB pre-flight gate):
  [`backend-inference-backends.md`](backend-inference-backends.md).
- **Persistence** (serde YAML round-trip for prompts, history index + eviction,
  model/user settings, recent-workspaces):
  [`backend-persistence.md`](backend-persistence.md).
- **Workspace UI** (tree, editor, run/stop, history panel):
  [`frontend-workspace.md`](frontend-workspace.md).
- **Settings UI** (storage path, temperature, cache clear, hardware panel):
  [`frontend-support-features.md`](frontend-support-features.md).

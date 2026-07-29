# Backend — Persistence Layer

All on-disk stores for QuantaMind. Source: `backend/src/persistence/`. Every file here is a Tauri-free I/O leaf: it takes a `&Path` (a directory or a file), reads/writes/serializes, and returns an `AppResult`. It never calls a Tauri API and never resolves the OS config directory itself — that is the job of the thin command wrappers one layer up (`backend/src/commands/`), which pass the path in.

> Cross-references: eval commands & engine → `backend-eval-engine.md`; publish wire/auth → `backend-publish.md`; STT pipeline & engine → `backend-stt.md`; workspace prompts & app/model settings commands → `backend-prompt-workspace-system.md`.

---

## Overview

### Why a dedicated persistence layer

1. **Separation of concerns.** A store knows *how* to lay bytes on disk; it does not know *where* the app config dir is, what a Tauri `AppHandle` is, or what the UI wants. The command layer resolves `app.path().app_config_dir()` (or a workspace root) and hands a concrete `&Path` to the store. This keeps every store unit-testable against a `tempdir()` with zero Tauri mocking — every file ships its own `#[cfg(test)]` round-trip.
2. **All disk I/O lives in Rust; the frontend passes only paths.** React never reads a file's contents. It passes a *path* (a picked file, a workspace node) to a command; Rust reads, **size-caps**, validates, and returns typed data. This is the trust boundary — a user-picked giant or malicious file is gated in one place. (See `MEMORY.md` → "Tauri file I/O belongs in Rust".)
3. **Robustness, uniformly applied.** Every reader enforces a `MAX_BYTES` cap so a corrupt/huge file can't OOM the process. A *missing* file is an empty/`None` result, never an error. Writes that can clobber existing history are **atomic** (temp-write + `rename`). Append-only logs **self-heal** a torn trailing line.

### What is stored where

All paths are under the OS app-config dir (`app.path().app_config_dir()`) unless noted. The shared GGUF/MLX weights and STT scratch live under `~/.quantamind/` instead.

| Store | On-disk location | Format | Owning feature |
| --- | --- | --- | --- |
| App user settings | `user_settings.yaml` | YAML (single struct) | Settings / onboarding |
| Per-model settings | `model_settings.yaml` | YAML (`HashMap<model, {temperature}>`) | Settings |
| Recent workspaces | `recent_workspaces.yaml` | YAML (capped list) | Workspace picker |
| Prompt files | `<workspace>/**/*.quantamind.yaml` | YAML (one `PromptFile` each) | Prompt workspace |
| Prompt run history | `<workspace>/.quantamind/history.yaml` + `runs/` blobs | YAML index | Prompt workspace |
| Eval collections | `evals/<name>.json` | JSON (`Vec<ToolTask>`) | Eval engine |
| Eval run history | `history/<collection>.json` | JSON (`Vec<RunSummary>`, capped 100) | Eval / regression timeline |
| Eval traces | `traces/<collection>.json` | JSON (`StoredCollectionTraces`) | Pipeline visualizer |
| Batch job logs | `jobs/<run_id>.jsonl` | JSONL (header + units) | Batch eval (resume) |
| Agentic transcripts | `agentic_transcripts/<collection>/<model>--<task>[--native].jsonl` | JSONL (steps + outcome) | Batch eval (post-mortem) |
| Batch reports | `batch_reports/<collection>.json` | JSON (`BatchReport`) | Readiness page |
| Readiness profiles | `readiness/<id>.json` | JSON (`ReadinessProfile`) | Readiness gating |
| Context Stress Test status | `cliff/<collection>.json` | JSON (`{model: CliffStatus}`) | Readiness / Context Stress Test |
| STT transcripts | `transcripts/<id>.json` | JSON (`Transcript`) | STT |
| STT eval specs | `stt_evals/<name>.json` | JSON (`SttEvalSpec`) | STT eval |
| STT eval reports | `stt_reports/<id>.jsonl` | JSONL (`SttReportRow`) | STT eval |
| STT readiness profiles | `stt_readiness/<id>.json` | JSON (`SttReadinessProfile`) | STT readiness |
| Publish rows | (no on-disk store) | in-memory → canonical wire JSON | Publish |

### How it's consumed (store → command module)

| Store module | Consuming command(s) | Dir/file resolved by |
| --- | --- | --- |
| `user_settings` | `commands/settings/user_settings.rs`, `system/onboarding.rs` | `app_config_dir/user_settings.yaml` |
| `model_settings` | `commands/settings/model_settings.rs` | `app_config_dir/model_settings.yaml` |
| `workspaces` | `commands/workspace/workspaces.rs` | `app_config_dir/recent_workspaces.yaml` |
| `prompts::io` / `prompts::tree` / `prompts::schema` | `commands/workspace/workspace_prompts.rs` | a workspace root (a user-picked folder) |
| `prompts::history` | `commands/workspace/history.rs` | `<workspace>/.quantamind/history.yaml` |
| `evals` | `commands/eval/eval_registry.rs`, `toolcall_cmd.rs`, import cmds | `app_config_dir/evals/` |
| `eval_history` | `commands/eval/matrix_cmd.rs`, `batch_cmd.rs` | `app_config_dir/history/` |
| `eval_trace_store` | `commands/eval/toolcall_cmd.rs` | `app_config_dir/traces/` |
| `jobs::queue` | `commands/eval/batch_cmd.rs` | `app_config_dir/jobs/` |
| `jobs::transcripts` | `commands/eval/batch_cmd.rs` (`TauriBatchSink`) | `app_config_dir/agentic_transcripts/` |
| `readiness::reports` | `commands/eval/batch_cmd.rs`, `readiness_cmd.rs` | `app_config_dir/batch_reports/` |
| `readiness::profiles` | `commands/eval/readiness_cmd.rs` | `app_config_dir/readiness/` |
| `readiness::cliff` | `commands/eval/readiness_cmd.rs` | `app_config_dir/cliff/` |
| `publish::row` / `canonical` / `validate` | `commands/publish/publish_cmd.rs`, `preview_cmd.rs` | n/a (built from a `ModelVerdict`) |
| `stt::transcripts` | `commands/stt/transcribe.rs`, `stt/eval/*` | `app_config_dir/transcripts/` |
| `stt::eval_specs` | `commands/stt/eval/eval_cmd.rs` | `app_config_dir/stt_evals/` |
| `stt::eval_reports` | `commands/stt/eval/eval_cmd.rs` | `app_config_dir/stt_reports/` |
| `stt::eval_readiness` | `commands/stt/eval/readiness_cmd.rs` | `app_config_dir/stt_readiness/` |

---

## Key cross-cutting patterns

These recur across the layer; the per-file sections reference them rather than re-explaining each time.

- **Validate untyped JSON via a strict struct.** Never traverse a `serde_json::Value` by hand. Deserialize the raw text into a `#[derive(Deserialize)]` struct (`Transcript`, `BatchReport`, `RunSummary`, `Vec<ToolTask>`); serde's missing/extra-field handling *is* the validation. `cliff.rs` is the one deliberate exception — it reads `HashMap<String, Value>` first only so it can migrate a *legacy bare-number* shape into the tagged `CliffStatus` enum. (See `MEMORY.md` → "Validate JSON via strict struct".)
- **Size cap before read.** Each reader calls `std::fs::metadata(path)?.len()` and rejects over `MAX_BYTES` *before* `read_to_string`. Caps are tuned per store: settings/history/profiles/reports = 1 MB, cliff = 256 KB, transcripts = 8 MB, job logs = 32 MB.
- **Missing ≠ error.** A non-existent file/dir returns the empty value (`default`, `Vec::new()`, `None`, empty map). Only an *over-cap* or *corrupt* file errors.
- **Atomic write for clobber-prone stores** (`cliff`, `transcripts`): write `*.json.tmp`, then `std::fs::rename` over the target — an OS-atomic swap. A crash mid-write leaves only the inert `.tmp`; the live file is always 100% old or 100% new.
- **Append-only JSONL with tail-healing** (`jobs/queue`, `stt/eval_reports`): each record is one line appended via `OpenOptions::append`. A crash can only truncate the final line; `load` discards a half-written trailing line and stops.
- **Filename safety, two flavours.** `evals::sanitize_name` *rejects* anything that isn't a bare stem (legacy; renaming it would orphan saved files). `readiness::safe_filename` *maps* any id to a bounded collision-proof stem (`≤40-char slug` + `-` + 8 hex of the full id) — used by all newer stores.

---

## File: `mod.rs`

**Responsibility:** module index. **What:** `pub mod` for the 7 top-level stores plus the `jobs`, `prompts`, `publish`, `readiness`, `stt` concern sub-folders (each a folder to keep `persistence/` under the ≤10-files folder-taxonomy budget).

---

## File: `user_settings.rs`

**Responsibility:** app-wide user preferences. **Why:** one place for cross-phase prefs that must survive every launch. **What:** `UserSettings { theme, first_run_complete, last_update_check_at, models_folder, stt_engine_dir }`; `load`/`save` (YAML). Every field is `#[serde(default)]` and skip-serialized when empty, so the file stays minimal and forward/backward compatible as fields are added. **How/Where used:** `commands/settings/user_settings.rs` (theme, models folder), `system/onboarding.rs` (`first_run_complete`); `stt_engine_dir` is consulted first by `whisper_dir` discovery (see `backend-stt.md`). Missing/empty file → `UserSettings::default()`.

---

## File: `model_settings.rs`

**Responsibility:** per-model inference temperature. **What:** `ModelSettings { temperature: f32 }`, `type ModelSettingsMap = HashMap<String, ModelSettings>`; YAML `load`/`save`. **How/Where used:** `commands/settings/model_settings.rs`. Missing/empty file → empty map. `save` `create_dir_all`s the parent.

---

## File: `workspaces.rs`

**Responsibility:** the MRU list of recently opened workspace folders. **What:** `RecentEntry { path, opened_at }`, `RecentList { entries }`, `MAX_RECENTS = 10`; YAML `load`/`save`, plus the pure helper `record(&mut list, entry)` — moves to front, dedupes by `path`, truncates to 10. **How/Where used:** `commands/workspace/workspaces.rs` → `recent_workspaces.yaml`.

```rust
pub fn record(list: &mut RecentList, entry: RecentEntry) {
    list.entries.retain(|e| e.path != entry.path);
    list.entries.insert(0, entry);
    list.entries.truncate(MAX_RECENTS);
}
```

---

## File: `eval_history.rs`

**Responsibility:** the regression timeline — one append-only score log per collection. **Why:** the unit the timeline plots over time. **What:** `RunSummary { ts, model, backend, parse_rate, tool_selection_acc, arg_acc, abstain_acc, composite, n, pass_k, agentic_avg_steps, effort }` (post-Phase-6 fields are `#[serde(default)]` so old single-turn history still loads). `MAX_ENTRIES = 100`, `MAX_BYTES = 1 MB`. `load(dir, collection_id)` (missing → empty), `append(dir, id, &[RunSummary])` — loads, extends, drains oldest past 100, writes pretty JSON. Filename via `evals::sanitize_name`. **How/Where used:** written by matrix & batch eval runs; read by the timeline (`backend-eval-engine.md`).

```rust
pub fn append(dir: &Path, collection_id: &str, new: &[RunSummary]) -> AppResult<()> {
    let mut entries = load(dir, collection_id)?;
    entries.extend_from_slice(new);
    if entries.len() > MAX_ENTRIES {
        entries.drain(0..entries.len() - MAX_ENTRIES); // keep newest 100
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(history_path(dir, collection_id)?, serde_json::to_string_pretty(&entries)?)?;
    Ok(())
}
```

---

## File: `eval_trace_store.rs`

**Responsibility:** cache every model's most-recent per-task traces for one collection, so "View Trace" never re-runs inference. **What:** `StoredCollectionTraces { models: Vec<ModelTraces> }`, `ModelTraces { model, backend, tasks: Vec<TaskTrace> }`; `MAX_BYTES = 1 MB`. `upsert(dir, collection_id, model, backend, &[TaskTrace])` merges by task id — replaces an existing task's trace, keeps the rest, refreshes the model's `backend`. Serves both the whole-collection Matrix write and the Simulator's incremental one-task-at-a-time streaming. `load_one(dir, collection, model, task_id) -> Option<TraceResult>` lets the visualizer fall back to a live run when nothing was saved. **How/Where used:** `commands/eval/toolcall_cmd.rs` → `traces/`.

---

## File: `evals.rs`

**Responsibility:** the custom eval-collection registry — JSON task files. **Why:** the source of truth for what gets evaluated; also the trust-boundary primitive for *all* file reads (collection load and CSV import both go through it). **What:** `MAX_BYTES = 1 MB`; `sanitize_name(name)` (rejects empty / `/` / `\` / `..` / leading `.`); `list(dir)` (sorted stems, missing → empty); `read_text_capped(path)` (cap-then-read raw text — the primitive the frontend never bypasses); `read_capped(path)` (cap → parse → `validate_tasks`); `load`/`save`/`delete`. `save` is additionally the **semantic write boundary**: after structural `validate_tasks` it runs `oracle::semantic_findings` (the world-state authoring contract — orphaned entity ids, expected getters that ack, unfetched oracle keys) and hard-rejects with an error naming each task + defect. Write-side only by design: `load`/`parse_collection` stay permissive so a pre-existing broken file can be opened and fixed. **How/Where used:** `commands/eval/eval_registry.rs`, import commands (import writes via `save`, so uploads hit the same boundary). `sanitize_name` is reused by `eval_history` and `eval_trace_store` and `stt::eval_specs` to key their files.

```rust
pub fn sanitize_name(name: &str) -> AppResult<String> {
    let t = name.trim();
    let bad = t.is_empty() || t.contains('/') || t.contains('\\')
           || t.contains("..") || t.starts_with('.');
    if bad { return Err(AppError::Validation(format!("invalid collection name: {name:?}"))); }
    Ok(t.to_string())
}
```

---

## Folder: `jobs/` — resumable batch eval queue

### File: `jobs/mod.rs`
Concern sub-folder doc: an append-only `.jsonl` log per run under `jobs/`; a leftover log == an interrupted run, loaded with truncated-tail healing and resumed.

### File: `jobs/queue.rs`
**Responsibility:** persist a batch eval run as a self-contained, resumable log. **Why:** the log is the *only* source of truth on resume — it carries the full work-list so resume never depends on the frontend re-sending anything. **What:**
- `RunConfig { collection_id, targets: Vec<ModelTarget>, tasks: Vec<ToolTask>, k, max_steps, params, keep_alive, native }` — everything needed to rebuild the work-list.
- `enum JobRecord { Header(RunConfig), Unit(CompletedUnit) }` (`rename_all="snake_case"`) — one per `.jsonl` line: header is line 1, each completed unit appended after.
- `MAX_BYTES = 32 MB` (full task specs in the header → generous cap).
- `run_path(dir, run_id)` keyed by `safe_filename`; `create` (truncate + write header), `append(unit)` (O(1) OS-atomic append), `load` (header + units, **healing** a torn tail), `delete`, `list_paths` (every leftover `.jsonl`).

```rust
pub fn load(path: &Path) -> AppResult<Option<(RunConfig, Vec<CompletedUnit>)>> {
    // ... exists / size-cap checks ...
    let mut header = None; let mut units = Vec::new();
    for line in BufReader::new(File::open(path)?).lines() {
        let Ok(line) = line else { break };        // I/O error on tail → stop
        if line.trim().is_empty() { continue; }
        match serde_json::from_str::<JobRecord>(&line) {
            Ok(JobRecord::Header(c)) => header = Some(c),
            Ok(JobRecord::Unit(u))   => units.push(u),
            Err(_) => break,                        // truncated final line → heal: drop + stop
        }
    }
    Ok(header.map(|c| (c, units)))
}
```

**How/Where used:** `commands/eval/batch_cmd.rs`. On start a unit not present in `units` simply re-runs — a torn tail is "not done."

### File: `jobs/transcripts.rs`
**Responsibility:** the durable per-(model, task) agentic transcript — every `TrajectoryStep` (raw model output, injection, step kind, env snapshot, D9 usage fields) plus the terminal `TaskOutcome`, one JSONL line each. **Why:** the live `EVENT_AGENTIC_STEP` stream is the UI's copy and vanishes with the window; before this store there was NO on-disk artifact to post-mortem a failing run turn-by-turn (the single biggest observability gap in the agentic path). **What:**
- `transcript_path(dir, collection_id, model, task_id, native)` → `agentic_transcripts/<collection>/<model>--<task>[--native].jsonl`; every segment through `safe_filename` (model names carry `:`), native and prompt passes get separate files. **`agentic_transcripts/`, NOT `transcripts/` — that dir is the STT feature's store.**
- `begin_task` (truncate/create — retention is **latest batch only**, the `eval_trace_store` most-recent-only philosophy; disk bounded by collection × models), `append_step` / `append_outcome` (O(1) `OpenOptions::append`, the `queue.rs` pattern), `read` (size-capped, `MAX_READ_BYTES = 32 MB`).
- `enum TranscriptRecord<'a> { Step(&TrajectoryStep), Outcome(&TaskOutcome) }` (`rename_all="snake_case"`) — each line self-describing (`{"step":…}` / `{"outcome":…}`).

**How/Where used:** written best-effort by `TauriBatchSink` (`batch_cmd.rs`) — `task_started` → `begin_task`, `agentic_turn` → `append_step`, `task_done` → `append_outcome`; a write failure prints a loud `[batch] WARN` and the run continues (a disk hiccup must never kill a batch, but is never silent). No read IPC/UI yet — the viewer is deferred (`docs/process.md#future-considerations`); inspect with any text tool.

---

## Folder: `prompts/` — workspace prompt files

### File: `prompts/mod.rs`
Index: `history`, `io`, `schema`, `tree`.

### File: `prompts/schema.rs`
**Responsibility:** the prompt-file serde contract. **What:**
- `InferenceParams { temperature, top_p, top_k, max_tokens, repeat_penalty, seed, num_ctx }` — all `Option`, skip-serialized when `None`. Reused by `prompts::history` and `jobs::queue`. **Defined in the domain** (`inference/params.rs`, so the eval engine can stamp it on `BatchReport` without breaching the layering law) and re-exported here for the storage/IPC call sites.
- `PromptFile { name, system, user, model?, params, created_at, updated_at, auto_rerun }`. **Why this shape:** `params` is **read-tolerant but never written back** (`#[serde(default, skip_serializing)]`) — global params are now the single source (frontend `paramsStore`); an old file with a `params` block still loads but loses it on next save.

```rust
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PromptFile {
    pub name: String,
    #[serde(default)] pub system: String,
    #[serde(default)] pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub model: Option<String>,
    #[serde(default, skip_serializing)] pub params: InferenceParams, // legacy read-only
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "is_false")] pub auto_rerun: bool,
}
```

### File: `prompts/io.rs`
**Responsibility:** read/write/delete/rename one `*.quantamind.yaml` prompt file + the path-safety guard. **What:** `EXT = "quantamind.yaml"`; `read` (missing → `NotFound`), `write` (`create_dir_all` parent), `delete` (file or dir), `rename` (refuses if target exists), and the security primitive `ensure_within(root, candidate)`.

```rust
/// Reject paths that escape `root` via `..` or symlinks; returns the canonical path.
pub fn ensure_within(root: &Path, candidate: &Path) -> AppResult<PathBuf> {
    let root_abs = root.canonicalize()?;
    let parent_abs = candidate.parent().unwrap_or(Path::new("/")).canonicalize()?;
    if !parent_abs.starts_with(&root_abs) {
        return Err(AppError::Validation(format!("path escapes workspace: {}", candidate.display())));
    }
    let name = candidate.file_name().ok_or_else(|| AppError::Validation("missing file name".into()))?;
    Ok(parent_abs.join(name))
}
```

**How/Where used:** `commands/workspace/workspace_prompts.rs` — every prompt path from the frontend is run through `state.ensure_within(...)` before any `io::read`/`io::write`.

### File: `prompts/tree.rs`
**Responsibility:** render the workspace folder as a sidebar tree of prompt files. **What:** `enum TreeNode { File { name, path } | Folder { name, path, children } }` (`tag="kind"`, `snake_case`). `list(root)` validates it's a dir then `walk`s recursively: skips the `.quantamind` hidden dir, includes only files ending `.quantamind.yaml`, **prunes empty folders**, sorts folders-before-files then by name. **How/Where used:** `workspace_prompts.rs::list` → the file explorer pane.

```rust
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TreeNode {
    File   { name: String, path: String },
    Folder { name: String, path: String, children: Vec<TreeNode> },
}
```

### File: `prompts/history.rs`
**Responsibility:** the per-workspace prompt-run history index. **What:** `HistoryEntry { id, name, prompt_path?, model, system, user, params, output_preview, output_len, token_count, ttft_ms?, tokens_per_sec?, load_ms?, ran_at }`, `History { entries }`; `MAX_HISTORY = 50`, `PREVIEW_CHARS = 280`. `record(&mut h, entry) -> Vec<evicted>` (newest-first, cap 50, **returns evicted so the caller deletes their output blobs**); `preview(s)`; `remove_by_path(&mut h, path) -> Vec<removed>`. **How/Where used:** `commands/workspace/history.rs` → `<workspace>/.quantamind/history.yaml`; full outputs live as separate blobs under `<workspace>/.quantamind/runs/` (the index here keeps only a 280-char preview + length), and the evicted/removed return values drive blob cleanup.

---

## Folder: `publish/` — leaderboard wire record

### File: `publish/mod.rs`
Phase 8 publish canonical record — a pure Tauri-free leaf. Builds the deterministic wire structure the closed server validates: metrics-only rows, sorted-key canonical JSON, a SHA-256 integrity hash, local pre-validation. `cohort_key` derivation lives in `commands/publish` (needs `HardwareSnapshot`), keeping this a dependency leaf. **No on-disk store** — these types build the HTTP payload (see `backend-publish.md`).

### File: `publish/row.rs`
**Responsibility:** the exact, minimal wire shape — metrics only, no task content ever. **What:** `PublishMetrics { pass_k, effort?, avg_steps? }`, `PublishRow { model, quant, cohort_key, tool_version, metrics }`. `PublishRow::project(verdict, cohort_key, tool_version) -> Option<PublishRow>` — returns `None` when `pass_k` or `quantization` is unmeasured, so an unmeasured row is *excluded* rather than sent as a null that would skew server baselines (client half of the null-poisoning guard).

```rust
pub fn project(v: &ModelVerdict, cohort_key: String, tool_version: &str) -> Option<PublishRow> {
    let pass_k = v.pass_k?;                 // unmeasured → excluded, not null
    let quant  = v.quantization.clone()?;
    Some(PublishRow {
        model: v.model.clone(), quant, cohort_key,
        tool_version: tool_version.to_string(),
        metrics: PublishMetrics { pass_k, effort: v.effort, avg_steps: v.avg_steps },
    })
}
```

### File: `publish/canonical.rs`
**Responsibility:** deterministic JSON + integrity hash. **What:** `canonicalize(v)` recursively re-emits every object with keys sorted (via `BTreeMap`) so two clients building the same logical batch produce byte-identical JSON; `canonical_json(rows)` (sorted keys, no whitespace); `canonical_hash(rows)` (lowercase-hex SHA-256 over the canonical JSON). TLS + bearer token + nonce + this hash close transit tampering.

### File: `publish/validate.rs`
**Responsibility:** locally re-run the *same* plausibility checks the server enforces, so a malformed row never enters the batch. **What:** `pre_validate(rows) -> Result<(), (usize, String)>` — returns the index of the first bad row + a field-named reason (mirrors the server's 422-with-index): non-empty `model`/`quant`/`cohort_key`, `pass_k ∈ 0..=1` and non-NaN, `effort > 0` when present, `avg_steps >= 0` when present. Decorative by design; authoritative validation is server-side.

---

## Folder: `readiness/` — Phase 7 readiness stores

### File: `readiness/mod.rs`
Flat-file persistence for readiness: editable profiles, last batch report per collection, cliff status — all keyed by `safe_filename` so long nested ids never truncate into colliding files.

### File: `readiness/safe_filename.rs`
**Responsibility:** map an arbitrary id to a collision-proof, bounded, path-safe stem. **What:** `safe_filename(id)` — lowercase, non-alphanumerics → `-`, take ≤40 chars, trim `-`, then suffix `-{8-hex of DefaultHasher over the FULL id}`. Two distinct ids sharing a 40-char prefix still get distinct stems. **Deliberately not** `evals::sanitize_name` — switching the eval stores would re-key and orphan every saved file, so this is only for the newer readiness/STT stores. Reused by `jobs::queue`, `readiness::{cliff,profiles,reports}`, `stt::{transcripts,eval_reports,eval_readiness}`.

```rust
pub fn safe_filename(id: &str) -> String {
    let slug: String = id.to_lowercase().chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let prefix = slug.chars().take(40).collect::<String>();
    let prefix = prefix.trim_matches('-');
    let mut h = DefaultHasher::new(); id.hash(&mut h);
    format!("{}-{:08x}", prefix, h.finish() as u32)
}
```

### File: `readiness/profiles.rs`
**Responsibility:** editable readiness gating profiles (`ReadinessProfile`), built-ins seeded on first use. **What:** `MAX_BYTES = 1 MB`. `ensure_builtins(dir)` writes any `builtins()` profile whose file is absent (a user edit to a built-in id overwrites the seed and persists). `list` (seed-then-sort-by-name), `load`, `save` (rejects empty id), `delete`. The profile shape (gates the engine measures): `min_pass_k` (hard), `max_avg_steps`/`max_ms_per_step` (soft → Conditional), `min_context_tokens` (hard when `Some`), `forbid_infinite_loop`, `forbid_hallucinated_completion`, `require_full_vram`, `require_native_fc`. **How/Where used:** `commands/eval/readiness_cmd.rs` → `readiness/`.

```rust
pub fn list(dir: &Path) -> AppResult<Vec<ReadinessProfile>> {
    ensure_builtins(dir)?;                       // first-run seed
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") { out.push(read_profile(&path)?); }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
```

### File: `readiness/reports.rs`
**Responsibility:** the most-recent batch report per collection (last-write-wins). **Why:** Rust is the source of truth for the verdict — the GUI command and a future CLI read the same bytes. **What:** `MAX_BYTES = 1 MB`; `save(dir, &BatchReport)` (keyed by `report.collection_id` via `safe_filename`), `load(dir, collection_id) -> Option<BatchReport>` (missing → empty state). **How/Where used:** `commands/eval/batch_cmd.rs` & `readiness_cmd.rs` → `batch_reports/`.

### File: `readiness/cliff.rs`
**Responsibility:** per-model Context Stress Test status for a collection. **What:** `MAX_BYTES = 256 KB`. Stores `{model: CliffStatus}` where `CliffStatus = NotProbed | NoCliff{tested} | Collapsed{depth} | …`. **Model keys are stored verbatim** (Ollama names carry colons); only the *filename* is sanitized. `load` reads `HashMap<String, Value>` first so `status_from_value` can **migrate a legacy bare number** into `Collapsed{depth}` (the only thing the old format recorded), otherwise deserializing the tagged enum. `save` is **atomic** (load → merge one model → temp-write → rename, last-write-wins per model). **How/Where used:** `commands/eval/readiness_cmd.rs` → `cliff/`.

```rust
fn status_from_value(v: &Value) -> Option<CliffStatus> {
    match v {
        Value::Number(n) => n.as_u64().map(|d| CliffStatus::Collapsed { depth: d as u32 }), // legacy
        other => serde_json::from_value(other.clone()).ok(),                                 // tagged enum
    }
}

pub fn save(dir: &Path, collection_id: &str, model: &str, status: CliffStatus) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    let mut map = load(dir, collection_id)?;
    map.insert(model.to_string(), status);                      // verbatim key, last-write-wins
    let final_path = cliff_path(dir, collection_id);
    let tmp = final_path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&map)?)?;
    std::fs::rename(&tmp, &final_path)?;                        // OS-atomic swap
    Ok(())
}
```

---

## Folder: `stt/` — speech-to-text artifacts

### File: `stt/mod.rs`
The STT I/O leaf: canonical `Transcript` JSON is the source of truth (text/SRT/VTT are derived exports, never this). The P4 eval leaves store eval specs, streamed report rows, readiness profiles; the dumb scorer reads stored transcripts and streams rows back (see `backend-stt.md`).

### File: `stt/transcripts.rs`
**Responsibility:** the canonical transcript store + lightweight summaries for the eval editor. **What:** `MAX_BYTES = 8 MB` (long audio). `TranscriptSummary { id, model, text }` (joined, trimmed segment text). `list_summaries(dir)` — sorted by id, **skips** an unreadable/over-cap file rather than failing the whole list. `save(dir, &Transcript)` — **refuses an incomplete transcript** (a truncated run must never land as final) and writes **atomically** (temp + rename). `load(dir, id) -> Option<Transcript>` (size-capped). **How/Where used:** `commands/stt/transcribe.rs` writes; STT eval reads stored transcripts as the scorer's input.

```rust
pub fn save(dir: &Path, t: &Transcript) -> AppResult<()> {
    if !t.complete {
        return Err(AppError::Validation("refusing to persist an incomplete transcript".into()));
    }
    std::fs::create_dir_all(dir)?;
    let final_path = transcript_path(dir, &t.id);          // safe_filename(id)
    let tmp = final_path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(t)?)?;
    std::fs::rename(&tmp, &final_path)?;                   // atomic
    Ok(())
}
```

### File: `stt/eval_specs.rs`
**Responsibility:** stored STT eval specs (reference transcript + critical tokens per task). **What:** `MAX_BYTES = 1 MB`. `list`/`load`/`save`/`delete` of `SttEvalSpec`, keyed via `evals::sanitize_name` (bare stem). `load` and `save` both call `spec.validate()` (ids unique/non-empty) — an invalid spec is refused on save and on read. **How/Where used:** `commands/stt/eval/eval_cmd.rs` → `stt_evals/`.

### File: `stt/eval_reports.rs`
**Responsibility:** stream one report per spec, append-only JSONL, so a 1000-row sweep never holds every row/alignment matrix in memory. **What:** `start(dir, id)` (truncate/create), `append_row(dir, id, &SttReportRow)` (O(1) atomic append, flushed), `load(dir, id) -> Option<SttReport>` (**heals** a torn final line by discarding it). Keyed via `safe_filename`. **How/Where used:** `commands/stt/eval/eval_cmd.rs` → `stt_reports/`. Mirrors `jobs/queue`'s JSONL pattern.

### File: `stt/eval_readiness.rs`
**Responsibility:** editable STT readiness profiles, built-ins seeded on first list. **What:** `MAX_BYTES = 1 MB`; `ensure_builtins`/`list`/`load`/`save`/`delete` of `SttReadinessProfile` (e.g. `min_rtf`, `max_wer`). Same seed-on-first-list / edit-overwrites-seed / sort-by-name behaviour as `readiness::profiles`, keyed via `safe_filename`. **How/Where used:** `commands/stt/eval/readiness_cmd.rs` → `stt_readiness/`.

---

## Master table — every store, its serde root, its layout

| Store module | Serde root type | File / dir layout | Owning UI doc |
| --- | --- | --- | --- |
| `user_settings` | `UserSettings` | `user_settings.yaml` (one file) | `backend-prompt-workspace-system.md` |
| `model_settings` | `HashMap<String, ModelSettings>` | `model_settings.yaml` (one file) | `backend-prompt-workspace-system.md` |
| `workspaces` | `RecentList` | `recent_workspaces.yaml` (one file) | `backend-prompt-workspace-system.md` |
| `prompts::schema`/`io`/`tree` | `PromptFile`, `TreeNode` | `<workspace>/**/*.quantamind.yaml` | `backend-prompt-workspace-system.md` |
| `prompts::history` | `History` | `<workspace>/.quantamind/history.yaml` + `runs/` blobs | `backend-prompt-workspace-system.md` |
| `evals` | `Vec<ToolTask>` | `evals/<sanitized>.json` | `backend-eval-engine.md` |
| `eval_history` | `Vec<RunSummary>` | `history/<sanitized>.json` (cap 100) | `backend-eval-engine.md` |
| `eval_trace_store` | `StoredCollectionTraces` | `traces/<sanitized>.json` | `backend-eval-engine.md` |
| `jobs::queue` | `JobRecord` (Header/Unit) | `jobs/<safe>.jsonl` | `backend-eval-engine.md` |
| `jobs::transcripts` | `TranscriptRecord` (Step/Outcome lines) | `agentic_transcripts/<safe>/<safe>--<safe>.jsonl` | `backend-eval-engine.md` |
| `readiness::reports` | `BatchReport` | `batch_reports/<safe>.json` | `backend-eval-engine.md` |
| `readiness::profiles` | `ReadinessProfile` | `readiness/<safe>.json` | `backend-eval-engine.md` |
| `readiness::cliff` | `HashMap<String, CliffStatus>` | `cliff/<safe>.json` | `backend-eval-engine.md` |
| `publish::row` | `PublishRow` | (wire only, no file) | `backend-publish.md` |
| `stt::transcripts` | `Transcript` | `transcripts/<safe>.json` | `backend-stt.md` |
| `stt::eval_specs` | `SttEvalSpec` | `stt_evals/<sanitized>.json` | `backend-stt.md` |
| `stt::eval_reports` | `SttReportRow` (lines) | `stt_reports/<safe>.jsonl` | `backend-stt.md` |
| `stt::eval_readiness` | `SttReadinessProfile` | `stt_readiness/<safe>.json` | `backend-stt.md` |

---

## Data-flow walkthroughs

### (a) Save + load a workspace prompt (round-trip integrity)

1. **Save.** UI sends `{ path, PromptFile }` to `save_prompt` (`workspace_prompts.rs`). The command resolves the workspace root and calls `state.ensure_within(path)` → `prompts::io::ensure_within(root, candidate)`, which canonicalizes both and rejects anything escaping the root via `..`/symlink, returning a normalized `PathBuf`.
2. `prompts::io::write(target, &file)` `create_dir_all`s the parent and serializes the `PromptFile` to YAML. Note the legacy `params` block is `skip_serializing` — global params are the single source, so it is never written back.
3. **List.** `list_prompts` → `prompts::tree::list(root)` walks the folder, skips `.quantamind`, keeps only `*.quantamind.yaml`, prunes empty folders, returns the sorted `TreeNode` tree the explorer renders.
4. **Load.** UI sends the chosen `path`; the command re-runs `ensure_within`, then `prompts::io::read(target)` reads + deserializes. Because both write and read go through the identical serde `PromptFile` contract, the round trip is shape- and value-exact (an old file's `params` is the only field intentionally dropped).

### (b) Batch eval job queue persistence + resume

1. **Start.** `batch_cmd.rs` builds a `RunConfig` (collection, targets, full task specs, k, steps, params, native) and calls `jobs::queue::create(run_path, &config)`, which truncates any stale log and writes the `Header` line. `run_path` = `jobs/<safe_filename(run_id)>.jsonl`.
2. **Run.** After each model×task unit completes, the command calls `jobs::queue::append(path, &unit)` — a single O(1) OS-atomic `writeln!` + flush of a `Unit` line. No read-modify-write, so a crash can only tear the final line.
3. **Crash / restart.** On launch, `jobs::queue::list_paths(jobs_dir)` finds every leftover `.jsonl` (each == an interrupted run). `jobs::queue::load(path)` returns `(RunConfig, Vec<CompletedUnit>)`, **healing** a half-written trailing `Unit` by discarding it.
4. **Resume.** The command rebuilds the full work-list from `RunConfig.tasks × targets`, subtracts the already-present `CompletedUnit`s, and re-runs only the remainder (the healed/torn unit re-runs, since it was never "done"). On completion the run's `RunSummary`s are appended to `eval_history` and the `BatchReport` saved via `readiness::reports`; `jobs::queue::delete` removes the now-finished log.

### (c) Readiness profile save / load

1. **First list.** `readiness_cmd.rs::list_readiness_profiles` calls `readiness::profiles::list(readiness_dir)`. `ensure_builtins` writes each `builtins()` profile (e.g. `coding-agent`) whose `readiness/<safe_filename(id)>.json` is absent — first-run seeding. The dir is then read, each file size-capped + deserialized into `ReadinessProfile`, sorted by name.
2. **Edit + save.** The user tweaks a gate (e.g. `min_pass_k`) and saves. `readiness::profiles::save` rejects an empty id, then writes the JSON to `readiness/<safe_filename(id)>.json` — **overwriting the seeded built-in** with the user's copy at the same id, so the edit survives every future `ensure_builtins` (the file now exists, so it is not re-seeded).
3. **Use.** During a readiness verdict, `readiness::profiles::load(dir, id)` reads the (possibly user-edited) profile; the engine applies its hard/soft gates against the model's measured metrics. Same pattern mirrored exactly by `stt::eval_readiness` for STT profiles.

# Backend Core — IPC, Lifecycle & Shared Primitives

> Subsystem doc. Scope: the Rust **spine** of the Tauri app — the builder,
> command registration, managed state, lifecycle reaping, and the small shared
> primitives (`errors`, `sync`, `time_iso`, `emit`, `validation`, `metrics`).
> Per-feature subsystems are cross-referenced, not duplicated here.

## Overview

**Why this core exists.** QuantaMind is a Tauri 2 desktop app: a React/TS
frontend in a webview talks to a Rust process over Tauri's IPC bridge. Every
capability the UI exposes — pulling a model, running inference, scoring an eval,
recording audio — is a Rust `#[tauri::command]`. Something has to (a) *register*
those ~123 commands, (b) hold the *shared, long-lived state* they mutate
(cancellation tokens, running-server handles, in-memory settings), (c)
*guarantee* the spawned sidecar servers die when the app dies, and (d) provide
the *common vocabulary* — one error type, one event helper, one clock, one
poison-safe lock — that every command reuses. That is this core.

**What it does.**
- Builds the Tauri application (`lib.rs::run`): registers plugins, `.manage(...)`
  state, the `.setup(...)` startup hook, the `invoke_handler!` command table, and
  the `reap_on_exit` run-loop callback.
- Owns **process lifecycle robustness** (`commands/app_lifecycle.rs`): startup
  orphan sweep, signal reaper, graceful-exit reaper for the four sidecar servers.
- Defines the **shared primitives** every command imports: `AppError`/`AppResult`
  (`errors.rs`), `log_emit` (`commands/emit.rs`), `MutexExt::lock_recover`
  (`sync.rs`), `now_utc` (`time_iso.rs`), input validation (`validation/`), and
  streaming throughput/timeline/timing (`metrics/`).

**How it fits.** This is the *spine*. Feature modules (`eval`, `stt`, `models`,
`compare`, `publish`, …) hang off it: they each define commands + a state struct,
which `lib.rs` registers and manages. They all `return Result<_, AppError>`, emit
progress via `log_emit`, timestamp with `now_utc`, and recover poisoned locks
with `lock_recover`. The spine knows nothing about any feature's internals — it
only knows the *shape* of the contract.

---

## The IPC contract

How a frontend `invoke("cmd", args)` reaches Rust and how data flows back:

1. **Frontend call.** `invoke("run_prompt", { model, prompt, … })` (camelCase
   keys) crosses Tauri's IPC bridge to the backend.
2. **Routing.** `tauri::generate_handler![ … ]` in `lib.rs` built a static
   dispatch table at compile time. Tauri matches the command *name* to the
   registered `#[tauri::command] fn`, deserializes the JSON args into the
   function's typed parameters (serde), and injects framework parameters by
   *type* (see managed state below).
3. **Execution.** The command runs (sync or `async`, the latter on Tauri's
   tokio runtime). It returns `Result<T, AppError>`.
4. **Return.** `Ok(T)` → serialized to JSON and resolves the JS promise.
   `Err(AppError)` → serialized via the enum's `#[serde(tag="kind",
   content="message")]` shape and *rejects* the promise, so the frontend
   `catch` receives `{ kind, message }`.
5. **Streaming / progress.** Long commands don't return data incrementally;
   they **emit events** (`log_emit(&app, "event-name", payload)`) that the
   frontend subscribes to with `listen(...)`. The command's `Ok(())` only
   signals completion.

**Managed state.** Framework parameters are injected by *type*, never sent from
JS. A command signature like:

```rust
#[tauri::command]
pub async fn run_prompt(
    app: tauri::AppHandle,                       // emit events, read paths
    state: tauri::State<'_, RunState>,           // shared cancellation token
    settings: tauri::State<'_, ModelSettingsState>,
    model: String, prompt: String,               // ← these come from invoke args
    …
) -> Result<(), AppError> { … }
```

`AppHandle` and every `State<'_, T>` are supplied by Tauri at call time. A
`State<'_, T>` resolves to whatever `.manage(T::default())` registered for that
`T` in `lib.rs`. Because state is keyed by type, there is exactly **one**
instance of each managed struct for the whole process — that is how a `stop_*`
command cancels a run started by a different `start_*` call: both receive the
same `State<'_, RunState>`.

---

## File: `backend/src/main.rs`

**Responsibility:** binary entrypoint. · **Why:** Tauri splits the app into a
`lib` crate (so it can also target mobile/`cdylib`) and a thin `main`. · **What:**
`fn main() { quantamind_lib::run() }` plus the Windows-subsystem attribute that
suppresses the console window in release. · **How/Where used:** the only thing
the `quantamind` binary does.

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() { quantamind_lib::run() }
```

## File: `backend/src/lib.rs`

**Responsibility:** assemble and run the entire Tauri app. · **Why:** one
canonical place that wires plugins, managed state, the startup hook, the command
table, and the exit hook — the spine's "main()". · **What:** `pub fn run()`
chaining `tauri::Builder::default()` → `.plugin(...)` ×4 → `.manage(...)` ×17 →
`.setup(...)` → `.invoke_handler(generate_handler![ … ])` → `.build(...)` →
`.run(reap_on_exit)`. · **How/Where used:** called by `main.rs`; it *is* the
process.

```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::prompt::prompt::RunState::default())
        // … 16 more .manage(...) …
        .setup(|app| { /* orphan sweep + signal reaper + STT reconcile */ Ok(()) })
        .invoke_handler(tauri::generate_handler![ /* ~127 commands */ ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(commands::app_lifecycle::reap_on_exit);
}
```

Notes:
- **Plugins (4):** `dialog` (native file/message dialogs), `process` (relaunch /
  exit, used by updater), `shell` (open allow-listed URLs — see capabilities),
  `updater` (auto-update from the endpoint in `tauri.conf.json`).
- **Feature gating:** three publish commands are wrapped
  `#[cfg(not(feature = "enterprise"))]` (login/preview/publish), so an
  enterprise/air-gapped build compiles out the cloud surface. The offline image
  export (`save_readiness_image`) stays in every build.
- **`.setup` hook** runs once after build, before the window shows:
  `sweep_orphans()`, `install_signal_reaper(...)`, `reconcile_stt_dir(...)`
  (heal half-installed STT artifacts), `clear_scratch(...)` (drop last session's
  recording scratch). See lifecycle section.

## File: `backend/src/commands/mod.rs`

**Responsibility:** the module map for all command groups. · **Why:** enforces
the "split by concern" rule — one folder per concern, no `utils`. · **What:** 20
`pub mod` declarations (`app_lifecycle`, `audio`, `compare`, `emit`, `eval`,
`gguf`, `hf`, `llama`, `mlx`, `models`, `ollama`, `prompt`, `prompt_templates`,
`publish`, `remote`, `settings`, `storage`, `stt`, `system`, `workspace`).
`remote` holds the vLLM/SGLang health + `/v1/models` discovery commands (one
module, both backends). · **How/Where used:** every command path in `lib.rs`
(`commands::<group>::<file>::<fn>`) resolves through here.

---

## Managed state inventory

Every `.manage(...)` call in `lib.rs`. Each registers exactly one instance of
its type, retrievable anywhere as `tauri::State<'_, T>` or
`app.state::<T>()`. All wrap their data in a `Mutex` (poison-recovered via
`sync.rs` where appropriate).

| State struct | Inner | Purpose | Owning module / doc |
|---|---|---|---|
| `prompt::RunState` | `Mutex<Option<CancellationToken>>` | Cancel the single in-flight prompt run (`run_prompt`/`stop_prompt`). | `commands/prompt` |
| `models::models_pull::PullState` | `Mutex<HashMap<String,CancellationToken>>` | Per-model cancellation for concurrent Ollama pulls. | [backend-models-hf-gguf.md](backend-models-hf-gguf.md) |
| `hf::hf_install::HfInstallState` | `Mutex<Option<CancellationToken>>` | Cancel the active HuggingFace GGUF install. | [backend-models-hf-gguf.md](backend-models-hf-gguf.md) |
| `compare::CompareRunState` *(alias → `inference::compare::compare_state`)* | run handles | Track/stop A-vs-B compare runs. | [backend-compare.md](backend-compare.md) |
| `settings::model_settings::ModelSettingsState` | `Mutex<ModelSettingsMap>` + `Mutex<bool>` loaded flag | In-memory per-model settings (e.g. temperature), lazy-loaded from disk. | `commands/settings` |
| `ollama::ollama_start::OllamaStartState` | `Mutex<bool>` in-progress + `Mutex<Option<u32>>` started_pid | Track the `ollama serve` **we** spawned so we only kill our own daemon. | [backend-inference-backends.md](backend-inference-backends.md) |
| `llama::llama_server_types::LlamaServerState` | `Mutex<Option<RunningServer>>` | Handle to the spawned `llama-server` sidecar. | [backend-inference-backends.md](backend-inference-backends.md) |
| `mlx::mlx_server_types::MlxServerState` | `Mutex<Option<Running>>` | Handle(s) to the spawned `mlx_lm.server`. | [backend-inference-backends.md](backend-inference-backends.md) |
| `stt::stt_server_types::SttServerState` | `Mutex<Option<Running>>` | Handle to the spawned `whisper-server` sidecar. | [backend-stt.md](backend-stt.md) |
| `stt::stt_download::SttInstallState` | `Mutex<Option<CancellationToken>>` | Cancel the active STT model download. | [backend-stt.md](backend-stt.md) |
| `audio::capture::CaptureState` | `Mutex<Option<Active>>` (+ `Drop`) | Active mic capture stream; `Drop` stops it on app teardown. | [backend-stt.md](backend-stt.md) |
| `workspace::workspaces::WorkspaceState` | `Mutex<Option<PathBuf>>` | Currently-open workspace root path. | [backend-persistence.md](backend-persistence.md) |
| `settings::user_settings::UserSettingsState` | `Mutex<UserSettings>` + loaded flag | App-wide user settings, lazy-loaded. | [backend-persistence.md](backend-persistence.md) |
| `eval::batch_cmd::BatchRunState` | `Mutex<Option<CancellationToken>>` | Cancel the running batch eval. | [backend-eval-engine.md](backend-eval-engine.md) |
| `eval::readiness_cmd::CliffRunState` | `Mutex<Option<CancellationToken>>` | Cancel the running Context Stress Test. | [backend-eval-engine.md](backend-eval-engine.md) |
| `publish::auth_state::AuthState` | `Mutex<Option<String>>` | In-memory access token for publish (refresh token lives in OS keyring). | [backend-publish.md](backend-publish.md) |

Pattern: most state is either a **cancellation handle** (`Option<CancellationToken>`,
or a map for concurrency) letting a `stop_*` command interrupt a `start_*`/`run_*`
command, or a **running-process handle** (`Option<Running>`) so the lifecycle
reaper can kill spawned servers. Lazy-loaded settings carry a `loaded: Mutex<bool>`
guard so disk I/O happens once.

---

## Command groups

The `invoke_handler!` table registers **127** commands. Grouped by
`commands::<module>` (counts from the registration block):

| Group | # | What it does | Owning doc |
|---|---|---|---|
| `remote` | 4 | Remote vLLM/SGLang health + model discovery (both over `GET /v1/models`, bearer-authed). No start/stop — the servers are remote. | [backend-inference-backends.md](backend-inference-backends.md) |
| `eval` | 29 | Eval engine: load/run tasks, tool-call eval + trace, custom/builtin collections, matrix runs, batch (run/stop/resume/discard), readiness profiles + assess, Context Stress Test. | [backend-eval-engine.md](backend-eval-engine.md) |
| `stt` | 22 | Speech-to-text: whisper-server start/stop/health/env, model download/catalog/list/delete, transcribe + load transcript, STT eval CRUD + report, STT readiness profiles. | [backend-stt.md](backend-stt.md) |
| `workspace` | 15 | Workspace open/close/current/tree/recent, prompt file CRUD (load/save/create/rename/delete), run history (append/list/get/clear/remove). | [backend-persistence.md](backend-persistence.md) |
| `settings` | 7 | Model settings (get + set temperature), storage path get/validate, user settings get/set + resolve models folder. | `commands/settings`, [backend-persistence.md](backend-persistence.md) |
| `mlx` | 7 | MLX health, list/delete/install models, server start/stop/status. | [backend-inference-backends.md](backend-inference-backends.md) |
| `system` | 5 | Hardware snapshot, loaded models, Ollama RSS, Ollama health, onboarding-workspace scaffold. | `commands/system` |
| `hf` | 6 | HuggingFace search, repo files (+all), model card, install GGUF, cancel install. | [backend-models-hf-gguf.md](backend-models-hf-gguf.md) |
| `models` | 5 | Ollama list/inspect, KV-cache estimate, pull/cancel-pull. | [backend-models-hf-gguf.md](backend-models-hf-gguf.md) |
| `llama` | 5 | llama-server start/stop + health, list/delete llama models. | [backend-inference-backends.md](backend-inference-backends.md) |
| `storage` | 4 | Installed models + stats, remove model, clear cache (opt-in `include_models` also wipes the HF snapshot cache, keeping the auth token), disk usage. | `commands/storage` |
| `publish` | 4–5* | Save readiness image (always); preview/publish/login (gated out of enterprise builds). | [backend-publish.md](backend-publish.md) |
| `compare` | 3 | Run/stop compare, save compare report. | [backend-compare.md](backend-compare.md) |
| `audio` | 3 | Start/stop recording, recording level (VU). | [backend-stt.md](backend-stt.md) |
| `prompt` | 2 | Run/stop a single streamed prompt. | `commands/prompt` |
| `ollama` | 2 | Start/stop the Ollama daemon we own. | [backend-inference-backends.md](backend-inference-backends.md) |
| `gguf` | 2 | Inspect a GGUF file, install a local GGUF. | [backend-models-hf-gguf.md](backend-models-hf-gguf.md) |
| `prompt_templates` | 1 | List bundled prompt templates. | `commands/prompt_templates` |

\* `publish` registers 4 commands unconditionally + 3 under
`#[cfg(not(feature="enterprise"))]`; the count above reflects the non-enterprise
build's registered names in the table.

`app_lifecycle` and `emit` are in `commands/` but expose **no** `#[tauri::command]`s
— they are spine helpers (below), not part of the IPC table.

---

## Lifecycle & process reaping — `commands/app_lifecycle.rs`

**Responsibility:** guarantee the four spawned sidecar servers never outlive the
app, by any exit path. · **Why:** Tauri does **not** kill child processes when it
exits, and `RunEvent::ExitRequested` only fires on a *graceful* quit (Cmd+Q) — not
on a signal kill or a `tauri dev` rebuild SIGKILL. Without these guards a stale
`whisper-server`/`llama-server`/`mlx_lm.server`/owned-`ollama` keeps holding its
port (→ `EADDRINUSE` next launch) and unified memory. This module closes every
gap. · **How/Where used:** wired in three places in `lib.rs` — `sweep_orphans()`
and `install_signal_reaper(...)` in `.setup`, `reap_on_exit` passed to `.run(...)`.

The four guarded targets: `whisper-server`, `llama-server`, `mlx_lm.server`, and
the `ollama serve` *we* started (never a user's pre-existing daemon).

### Conservative identity (`is_our_server_cmd`)

Orphan killing must never touch a stranger's process. A process counts as ours
**only** if its command line both references our private dir marker *and* names
one of our server binaries:

```rust
const OUR_MARKER: &str = ".quantamind";
const SERVER_BINS: &[&str] = &["whisper-server", "llama-server", "mlx_lm.server"];

fn is_our_server_cmd(cmd: &str) -> bool {
    cmd.contains(OUR_MARKER) && SERVER_BINS.iter().any(|b| cmd.contains(b))
}
```

A user's own `whisper-server` pointed at *their* models dir fails the marker
check and is never killed (the test asserts exactly this).

### Three exit paths, three guards

**1. Graceful quit → `reap_on_exit` (run-loop callback).** Fires on
`RunEvent::ExitRequested`; calls `reap_managed`, which kills all four via their
managed-state handles. Idempotent.

```rust
pub fn reap_on_exit(app: &AppHandle, event: RunEvent) {
    if let RunEvent::ExitRequested { .. } = event { reap_managed(app); }
}
```

`reap_managed` reaches into managed state — `MlxServerState::kill_all_servers`,
`LlamaServerState::stop`, `SttServerState::stop`, `OllamaStartState::stop_owned`
— logging each failure (no silent swallow):

```rust
fn reap_managed(app: &AppHandle) {
    if let Err(e) = app.state::<MlxServerState>().kill_all_servers() { eprintln!("mlx reap failed: {e}"); }
    if let Err(e) = app.state::<LlamaServerState>().stop()           { eprintln!("llama reap failed: {e}"); }
    if let Err(e) = app.state::<SttServerState>().stop()             { eprintln!("whisper reap failed: {e}"); }
    if let Err(e) = app.state::<OllamaStartState>().stop_owned()     { eprintln!("ollama reap failed: {e}"); }
}
```

**2. Signal kill → `install_signal_reaper` (SIGINT/SIGTERM).** Ctrl+C, `kill`, or
a dev-tool restart sends a signal — `ExitRequested` does *not* fire. A spawned
tokio task `select!`s on both signals, reaps, then exits. `#[cfg(not(unix))]`
is a no-op.

```rust
#[cfg(unix)]
pub fn install_signal_reaper(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let (mut term, mut intr) = match (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) {
            (Ok(t), Ok(i)) => (t, i), _ => return,
        };
        tokio::select! { _ = term.recv() => {} _ = intr.recv() => {} }
        eprintln!("[reap] termination signal — stopping servers");
        reap_managed(&app);
        std::process::exit(0);
    });
}
```

**3. Crash / SIGKILL last time → `sweep_orphans` (startup).** A SIGKILL runs *no*
in-process hook, so the *next* launch must clean up. `sweep_orphans` scans all
processes via `sysinfo`, skips our own PID, and kills any `is_our_server_cmd`
match (`SIGTERM`, then hard `kill` if needed). Returns the count.

```rust
pub fn sweep_orphans() -> usize {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::everything());
    let me = std::process::id();
    let mut killed = 0;
    for proc in sys.processes().values() {
        if proc.pid().as_u32() == me { continue; }
        let cmd = proc.cmd().iter().map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ");
        if is_our_server_cmd(&cmd) {
            eprintln!("[reap] killing orphaned QuantaMind server: {cmd}");
            if proc.kill_with(Signal::Term).is_none() { proc.kill(); }
            killed += 1;
        }
    }
    killed
}
```

**Guarantee:** across all three exit modes (graceful, signal, crash) and any
combination, no QuantaMind-spawned server survives into the next session, and no
non-QuantaMind process is ever touched. (Disk-side healing — half-installed STT
files — is reconciled separately in `.setup` via `reconcile_stt_dir`, see
[backend-stt.md](backend-stt.md).)

---

## Errors — `backend/src/errors.rs`

**Responsibility:** the one error type every command returns. · **Why:** a single
serializable enum gives the frontend a stable, typed failure shape and one place
to map raw errors into friendly copy. · **What:** `AppError` (`thiserror::Error` +
`Serialize`) with variants `Validation`, `InvalidTaskSchema`, `NotFound`,
`Inference`, `Io`, `Timeout`, `AuthRequired`, `Internal`; alias
`AppResult<T> = Result<T, AppError>`; `From<io::Error>` and `From<serde_json::Error>`
for `?`-propagation; `fn friendly(&self) -> String` for actionable UI copy.
· **How/Where used:** every `#[tauri::command]` returns `Result<_, AppError>`; on
`Err` Tauri serializes it and rejects the JS promise.

The serde attribute fixes the wire shape the frontend `catch` receives:

```rust
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum AppError {
    #[error("validation: {0}")]      Validation(String),
    #[error("not found: {0}")]       NotFound(String),
    #[error("inference: {0}")]       Inference(String),
    #[error("auth required: {0}")]   AuthRequired(String),
    #[error("internal: {0}")]        Internal(String),
    // …Io, Timeout, InvalidTaskSchema…
}
```

So a JS rejection looks like `{ kind: "not_found", message: "…" }`. `friendly()`
upgrades known raw strings (e.g. "Connection refused"/"os error 61" → "Ollama is
not running…"; "model … not found" → install hint; OOM → smaller-model hint).

## Event emission — `backend/src/commands/emit.rs`

**Responsibility:** the single helper for pushing events to the webview. · **Why:**
emission can fail (webview gone); a dropped event must hit the logs, never vanish
silently (`docs/architecture.md#robustness`). · **What:** `log_emit<S: Serialize +
Clone>(app, event, payload)`. · **How/Where used:** every streaming/progress
command (prompt tokens, pull progress, batch-eval steps, cliff probe) calls it;
the frontend `listen("event")`s for the payloads.

```rust
pub fn log_emit<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(e) = app.emit(event, payload) { eprintln!("emit '{event}' failed: {e}"); }
}
```

## Lock recovery — `backend/src/sync.rs`

**Responsibility:** poison-safe mutex access. · **Why:** if a thread panics while
holding a lock, `.lock().unwrap()` would cascade the panic; usually the data is
fine and recovering it is correct. · **What:** `trait MutexExt<T> { fn
lock_recover(&self) -> MutexGuard<'_, T> }` for `Mutex<T>`. · **How/Where used:**
managed-state methods take their lock via `.lock_recover()` instead of
`.lock().unwrap()` — except metrics, where poison should yield a sentinel/zero
(documented exception in the source).

```rust
impl<T> MutexExt<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}
```

## Clock — `backend/src/time_iso.rs`

**Responsibility:** dependency-free UTC ISO-8601 timestamps. · **Why:** the locked
stack adds no `chrono`/`time` crate; persisted records (history, evals, publish)
need a stable `YYYY-MM-DDTHH:MM:SSZ` string. · **What:** `now_utc() -> String`,
`format_secs(i64) -> String`, and `civil_from_days` (Howard Hinnant's proleptic
Gregorian inverse). · **How/Where used:** anywhere a timestamp is written to disk
or into an event payload.

```rust
pub fn now_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format_secs(secs as i64)   // → "2023-11-14T22:13:20Z"
}
```

## Input validation — `backend/src/validation/`

**Responsibility:** namespace for IPC input validation. · **Why:** commands must
reject malformed args before acting (`docs/architecture.md#robustness`). · **What:**
currently `mod.rs` only — a placeholder declaring the concern (the prompt-input
schema lands per phase). Note: command-local validation (e.g. `validate_params`
in `prompt.rs`, the strict-struct JSON validation used by eval) lives next to the
commands that own those shapes; this folder is the home for *cross-command*
schema validation as it grows. · **How/Where used:** declared as `pub mod
validation` in `lib.rs`; reserved surface.

---

## Streaming metrics — `backend/src/metrics/`

The shared, backend-agnostic measurement of token streams. Used by prompt,
compare, and eval inference to report real (never fabricated) TTFT / throughput /
per-token timeline.

### File: `metrics/timing.rs`
**Responsibility:** accumulate one run's timing as tokens stream in. · **What:**
`struct RunTiming { start, first_token, last_token, token_count, timeline }`
with `start()`, `record_token(text)`, `ttft_ms() -> Option<u64>`,
`tokens_per_sec() -> Option<f64>`, `timeline() -> &[TokenTiming]`. Built on a
monotonic `Instant`. · **How/Where used:** an inference loop calls
`record_token` per chunk, then reads the metrics at the end.

```rust
pub fn record_token(&mut self, text: &str) {
    let now = Instant::now();
    if self.first_token.is_none() { self.first_token = Some(now); }
    self.last_token = Some(now);
    self.token_count += 1;
    self.timeline.push(TokenTiming { text: text.to_string(),
        t_ms: (now - self.start).as_millis() as u64, n: self.token_count as u32 });
}
```

### File: `metrics/throughput.rs`
**Responsibility:** the two pure math helpers. · **What:** `ttft_ms(Duration) ->
u64`; `tokens_per_sec(span: Duration, count: usize) -> Option<f64>` — returns
`None` on zero span or zero count (no fake numbers). · **How/Where used:** called
by `RunTiming`'s `ttft_ms`/`tokens_per_sec`.

```rust
pub fn tokens_per_sec(span: Duration, count: usize) -> Option<f64> {
    let secs = span.as_secs_f64();
    if secs <= 0.0 || count == 0 { return None; }
    Some(count as f64 / secs)
}
```

### File: `metrics/timeline.rs`
**Responsibility:** the per-token wire record. · **What:** `struct TokenTiming {
text: String, t_ms: u64, n: u32 }` (`Serialize`). Terse keys keep long-output
arrays small; `t_ms` is ms since run start, `n` is the 1-based cumulative count.
· **How/Where used:** pushed by `RunTiming::record_token`; serialized into run
results/events the frontend charts.

### File: `metrics/mod.rs`
Three `pub mod` lines (`throughput`, `timeline`, `timing`). Note the TTFT
Instant is *shared*: `timeline[0].t_ms == ttft_ms()` by construction (asserted in
tests).

---

## Inference module map — `backend/src/inference/mod.rs`

Out of scope to deep-dive (per-backend docs own the internals); included as the
map the spine sits above. `inference/mod.rs` declares 19 submodules:

`backend`, `chat`, `compare`, `create`, `eval`, `generate`, `gguf`, `hf`,
`http`, `llama`, `mlx`, `ollama`, `openai` (the shared OpenAI SSE codec),
`pull`, `sglang`, `stt`, `vllm`, plus two leaf files `token_handler.rs` and
`vram_math.rs`.

Relationship to commands: a `commands::<group>` module is the **thin** IPC layer
(deserialize args, manage state, emit events, map errors) that delegates the real
work to the matching `inference::<group>` module (the sink). E.g. `commands/prompt`
→ `inference/generate` + `metrics`; `commands/compare` → `inference/compare`
(which also defines `CompareRunState`). Backend selection (`BackendKind`,
endpoints) lives in `inference/backend`; details in
[backend-inference-backends.md](backend-inference-backends.md).

---

## Config & capabilities — `tauri.conf.json`, `capabilities/`, `Cargo.toml`

### `backend/tauri.conf.json`
App identity (`dev.quantamind.app`, v0.2.0), one `main` window (1000×700), and
bundling. Key points:
- **`build`:** dev runs the Vite frontend (`pnpm dev`, `devUrl :1420`); release
  builds `../frontend/dist`.
- **`bundle.resources`:** ships `binaries/` (sidecar servers), `../docs/prompts/`
  → `prompts/`, `../docs/evals/` → `evals/` as runtime assets — i.e.
  `docs/prompts` and `docs/evals` are *app assets*, not engineering docs.
- **`bundle.macOS`:** ad-hoc `signingIdentity: "-"` + `macos.entitlements`.
- **`createUpdaterArtifacts: true`** + **`plugins.updater`:** endpoint
  `https://quantamind.co/releases/latest.json` with a minisign `pubkey`.
- **`security.csp: null`** (no CSP set at this stage).

### `backend/capabilities/default.json`
The IPC capability that grants the `main` window its permissions. Allows the
**core**, **dialog**, **updater**, and **process** plugin defaults, plus a
*scoped* `shell:allow-open` restricted to an explicit URL allow-list
(`ollama.com/download`, `ollama.com/library`, `huggingface.co/**`,
`quantamind.co/**`, `mailto:info@quantamind.co**`). The shell open is the only
non-default-scoped grant — the app cannot open arbitrary URLs.

### `backend/Cargo.toml` — locked stack (summary)
- **Crate:** lib name `quantamind_lib`, types `staticlib`/`cdylib`/`rlib`
  (so `main.rs` and mobile both link it). `edition = 2021`.
- **Feature `enterprise`:** compiles out the entire publish/auth surface
  (`commands/publish/{auth,preview_cmd,publish_cmd,cohort}`); offline
  `export_cmd` always stays in.
- **Tauri:** `tauri 2` + plugins `dialog`, `process`, `shell`, `updater` (v2).
- **Serde:** `serde` (derive), `serde_json`, `serde_yaml`, `thiserror 2`.
- **Async/HTTP:** `tokio 1` (macros, rt-multi-thread, sync, fs, io-util,
  **signal** — used by the reaper), `tokio-util`, `futures-util`,
  `reqwest 0.12` (json, multipart, stream).
- **System/IDs/hash:** `sysinfo 0.32` (disk+system — used by orphan sweep),
  `uuid v4`, `sha2`, `bytes`.
- **Publish auth (non-enterprise):** `keyring 3` (OS secure store; Linux without
  a secret service falls back to in-memory), `base64 0.22` (PKCE S256).
- **Audio/STT:** `hound`, `rubato`, `symphonia` (mp3/pcm/wav), `cpal`,
  `webrtc-vad` (independent, non-ML VAD — anti-circularity for the STT
  silence-hallucination metric).
- **Dev-deps:** `mockito`, `tempfile`.

No logging/state-machine/UI-kit crates — consistent with the locked-stack rule
in `CLAUDE.md` / `docs/process.md#tech-stack`.

---

## Cross-references

- Eval engine & readiness/cliff: [backend-eval-engine.md](backend-eval-engine.md)
- Speech-to-text + audio capture: [backend-stt.md](backend-stt.md)
- Model browse/pull (Ollama/HF/GGUF): [backend-models-hf-gguf.md](backend-models-hf-gguf.md)
- Inference backends (Ollama/llama/MLX, server spawn): [backend-inference-backends.md](backend-inference-backends.md)
- Compare runs: [backend-compare.md](backend-compare.md)
- Publish / auth / export: [backend-publish.md](backend-publish.md)
- Persistence (workspaces, history, settings on disk): [backend-persistence.md](backend-persistence.md)
- Architecture law (layering, sink/thin-command, robustness): `../architecture.md`

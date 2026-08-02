# Architecture & Code Structure

Module boundaries, the dependency law (layering), the failure policy, and the
folder rules. Companion docs: `process.md` (how we work) and `reference.md`
(contracts + troubleshooting).

## Architecture

QuantaMind is a Tauri desktop app: React/TS frontend, Rust backend, JSON IPC,
HTTP to a local llama.cpp server.

### Mental model

```
┌─────────────────────────────────────────────────────────────┐
│                  QuantaMind Desktop App                     │
│                                                             │
│  ┌────────────────────────────────────────────────────┐    │
│  │            React + TypeScript Frontend             │    │
│  │  features/  ←  shared/ipc/  ←  Tauri invoke()      │    │
│  └──────────────────────────┬─────────────────────────┘    │
│                             │                              │
│                    IPC boundary (JSON)                     │
│                             │                              │
│  ┌──────────────────────────▼─────────────────────────┐    │
│  │               Rust Backend (backend/)              │    │
│  │  commands/  →  inference/  →  metrics/             │    │
│  │       ↓                                            │    │
│  │  persistence/                                      │    │
│  └──────────────────────────┬─────────────────────────┘    │
└─────────────────────────────┼──────────────────────────────┘
                              │ HTTP
                              ▼
                ┌─────────────────────────────┐
                │   llama.cpp (localhost:8081)  │
                └─────────────────────────────┘
```

### Module boundaries

**Frontend (`frontend/src/`)**

- `app/` — application shell, routing, providers. No feature logic.
- `features/<name>/` — self-contained vertical slice. Owns its components,
  hooks, state, types, schemas, and tests. Deletable in one `rm -rf`.
- `shared/ipc/` — only place that calls Tauri `invoke`. Typed wrappers.
- `shared/components/` — primitives reused by 2+ features. If only one
  feature uses it, it lives in that feature.

**Backend (`backend/src/`)**

- `commands/` — IPC entry points. Thin: validate, wire Tauri, delegate to a pure
  core. The **only** layer that names `tauri::` types. See [Layering](#layering).
  `run_prompt` is backend-aware (dispatches to llama.cpp or the `llama-server`
  sidecar per the request's `backend`); the workspace sidebar's backend list picks it.
  `commands/publish/` (Phase 8) holds the share/publish commands: `export_cmd` is a
  thin offline PNG sink (ships in every build); the auth + send surface
  (`auth`/`pkce`/`token`/`login_cmd`/`cohort`/`preview_cmd`/`publish_cmd`) is gated
  behind the `enterprise` cargo feature — `#[cfg(not(feature = "enterprise"))]` on the
  modules AND their `generate_handler!` entries — so it compiles OUT of enterprise/
  air-gapped builds. Auth uses PKCE (no client secret); the refresh token lives in the
  OS keychain (`keyring`) with an in-memory fallback when no secret service exists; the
  short-lived access token is the only managed `AuthState` (un-gated so `.manage()`
  works in every build). The pure, metrics-only canonical record + hash + local
  pre-validation live as a leaf in `persistence/publish/`.
- `os/` — **OS platform adapter** with strict per-OS files. One `EngineHost`
  trait (`engine_host.rs`) with three impls — `macos::MacosHost`,
  `linux::LinuxHost`, `windows::WindowsHost` — each in its own file
  `#[cfg(target_os = "…")]`-gated at the mod declaration, so a macOS build
  contains **zero** Windows/Linux code, a Windows build contains zero
  macOS/Linux code, and so on. `host.rs` type-aliases `Host` to the right impl
  via `cfg`, with a `compile_error!` for unsupported target OSes. Every
  lifecycle module (`commands/llama/llama_runtime.rs`, plus
  `commands/app_lifecycle.rs`) uses `os::Host::…` instead of scattering
  `#[cfg(target_os = "…")]` blocks. Methods: `resolve_on_path` (`which` on
  macOS/Linux vs `where.exe` on Windows), `envs_for_lib_dir`
  (`DYLD_FALLBACK_LIBRARY_PATH` on macOS, `LD_LIBRARY_PATH` on Linux, empty on
  Windows), `apply_spawn_flags` (Windows `CREATE_NO_WINDOW |
  CREATE_NEW_PROCESS_GROUP` — R1: the process-group flag is what makes
  `graceful_stop` target the child instead of QuantaMind itself; no-op on
  Unix), `graceful_stop` (SIGTERM on Unix vs `GenerateConsoleCtrlEvent
  (CTRL_BREAK_EVENT, pid)` on Windows), `hard_stop` (SIGKILL vs
  `TerminateProcess`), `pid_alive`. Also `user_dirs::data_dir` →
  `~/.quantamind` on Unix, `%LOCALAPPDATA%\QuantaMind` on Windows.

  Two cfg-neutral helpers sit here because **more than one subsystem spawns**, and
  both concerns are inherently OS work rather than any one caller's business:
  `proc_group` stops a child's whole **process group** (a server forks its own
  children — an MCP server is `npx`→`node` — so killing the pid we hold would
  orphan the grandchild), and `scratch_dir::ScratchDir` is a self-removing temp
  directory that also reaps the ones a SIGKILL'd run could never `Drop`. The
  sweep goes through `Host::pid_alive`, so it now works on **all three
  platforms** — it was previously `#[cfg(unix)]`-gated and Windows leaked every
  orphaned directory. It is prefix-namespaced (`qm-mcp-world`, `qm-cert`, …) so
  one subsystem's sweep never considers another's directories, and it skips any
  directory not owned by the current uid so a shared `/tmp` can't let one user
  delete another's live run.
  Cross-OS-testable helpers (pure fallback-path builders, JSON parsers) live
  in cfg-neutral files so their tests run on any CI runner; per-OS lifecycle
  code is only compiled + tested on its own OS runner (Phase 5 CI matrix
  covers all three). Adding a new engine = one adapter impl, no new cfg
  blocks in the caller sites. **Every** subprocess spawn — long-lived sidecars
  and short-lived diagnostic probes alike (`resolve_on_path`'s `where.exe`, the
  GPU vendor CLIs `nvidia-smi`/`rocm-smi`/`xpu-smi`, a sidecar's `--help` dry-run) —
  is built via `Host::command`, which pre-applies `apply_spawn_flags`. A spawn
  that skips it would pop a transient console window on a GUI-launched Windows
  app (the "flashing terminals on launch" bug), so bare `Command::new` is banned
  in shipped paths: a `disallowed_methods` clippy lint (`backend/clippy.toml`)
  denies it on Windows, forcing new spawns through `Host::command`.
- `inference/` — backend adapters behind the `InferenceBackend` trait
  (`backend.rs`). `LlamaCppBackend` (a bundled `llama-server` sidecar) and
  `VLlmBackend` (a remote GPU server) today; callers build one by
  matching `BackendKind` (a closed enum — no `dyn`/`async-trait`). Cloud adds
  another variant. Both sidecar backends have an **app-managed lifecycle**: the
  app spawns/kills the server (`commands/{llama,vllm}/…start`), reaps children on
  exit (`commands/app_lifecycle.rs`), and the vLLM server runs on a dynamic port
  resolved via `inference/vllm/server/vllm_endpoint.rs` — not a hardcoded `:8082`.
  **Tauri-free and must not import `crate::commands`** — when it must report
  progress it takes a sink trait (see [Layering](#layering)), not an `AppHandle`.
- `metrics/` — measurements: TTFT, tokens/sec, VRAM.
- `fs_guard/` — rule 7(b)'s path-confinement chokepoint (`ensure_within`). A top-level
  primitive over `std::path` + `errors`, beside `secrets` (rule 7(a)'s): it confines a path
  and stores nothing, so it is not a persistence leaf. Both the IPC layer (workspace I/O) and
  the domain (the MCP sandbox root) must reach it — and the domain may not import
  `persistence/` ([Layering](#layering)), which is exactly what filing it there violated.
- `persistence/` — YAML/JSON read+write of prompts and history, plus `evals.rs`
  (custom tool-call eval collections: one `.json` per collection, name-sanitised,
  size-capped, validated on every read/write). The shared GGUF weights folder
  resolves via `UserSettings.models_folder` → `storage_disk::gguf_dir_resolved`
  (`UserSettingsState::weights_dir`); HF + local installs land there for
  llama.cpp and import into llama.cpp when reachable. Per-OS default (Phase 4):
  `~/.quantamind/gguf` on Unix (via `os::user_dirs::data_dir()` — backwards
  compatible), `%LOCALAPPDATA%\QuantaMind\gguf` on Windows (no env-var
  gymnastics for a fresh install). `models_dir` (llama.cpp's on-disk model
  location) uses `dirs::home_dir()` so `%USERPROFILE%\.llama_cpp\models` resolves
  on Windows without setting `QUANTAMIND_GGUF_DIR`. `warn_on_legacy_windows_paths()`
  runs at startup and stderr-logs (never auto-moves) if a legacy
  `~/.quantamind/{gguf,vllm}` sits alongside the new Windows default —
  irreplaceable user weights are the reason.
- `validation/` — schemas. Shared by commands and persistence.
- `errors.rs` — single `AppError` enum. No `unwrap()` outside tests.

### Rules

1. **One file = one concern.** If you need "and" to describe what a file
   does, split it.
2. **No cross-feature imports.** Features talk to each other only via
   `shared/` or via the backend.
3. **IPC is the only Rust/TS bridge.** No code-gen, no shared types file —
   keep contracts explicit in `shared/ipc/types.ts` and mirror in Rust.
4. **Validation at boundaries.** Zod on the TS side, `validator` + serde on
   the Rust side. Never trust IPC payloads.
5. **Errors are typed.** Rust returns `Result<T, AppError>`. TS returns
   discriminated unions, not thrown errors across IPC.
6. **Hooks for ephemeral, store for shared.** Per-action state that lives
   only as long as the action (mid-run output, install progress, ongoing
   fetch) belongs in a hook's local `useState`. Cross-component state read by
   parts of the UI that don't drive the action (current model, list of
   installed models, last run's final metrics) belongs in the Zustand store.
   Hooks may write to the store at completion; components must not read both
   the hook's local state and the store for the same piece of data — pick one
   source per piece of data.
7. **App-shell selection is global state in `shared/state/`.** The backend
   (`backendStore`), the model selection (`selectedModelStore`), and the inference
   parameters (`paramsStore`) define "what am I running and how" for the whole
   app, surfaced in the global header. They are not owned by any feature slice —
   a feature must not own state every other feature reads. The model selection is
   an array: llama.cpp is multi-select (2+ → a compare), llama.cpp single. The
   model list is filtered to the selected backend; switching backend reconciles
   the selection imperatively inside `setSelectedBackend` (trims off-backend
   models), never via a cross-store subscription. Every page reads this global
   selection — there is no per-page model picker (Tests keeps its own batch-target
   multi-select, filtered to the backend).

Update this section when a new top-level module is added, a boundary rule
changes, or the IPC contract gains a new category of message.

---

## Layering

How the backend modules depend on each other, and the two patterns that keep the
domain layer pure and testable. See [Architecture](#architecture) for the module
list and [Robustness](#robustness) for the failure policy.

### The dependency law

Source dependencies point **inward, to the domain**. `inference/` is the pure core;
everything else depends on it, and it depends on nothing else in the crate. This is the
Dependency Rule (hexagonal / Ports & Adapters — see
[`../rust-engineering-architecture-guide.md`](../rust-engineering-architecture-guide.md)
and [`../ARCHITECTURE.md`](../ARCHITECTURE.md)).

```
commands/  ──►  inference/ (domain)  ◄──  persistence/ , metrics/
  (IPC,            pure core,              driven adapters: serialize
  driving)         defines ports          domain types, time streams
```

- `commands/` is the only layer that touches Tauri (`AppHandle`, `State`,
  `Emitter`, `#[tauri::command]`). It is the *driving adapter*.
- **`inference/` must be Tauri-free.** It must not import `crate::commands`, must not
  import `crate::persistence`, and must not name any `tauri::` type. If domain code
  needs to report progress it takes a **sink** (below), not an `AppHandle`. If it shares
  a type with persistence, that type lives *in the domain* and persistence imports it
  (e.g. `RunSummary` in `inference/eval/run_summary.rs`).
- `persistence/` and `metrics/` are **driven adapters**: plain data in,
  `Result<T, AppError>` out. They legitimately depend on domain *types* (to serialize a
  `ReadinessVerdict`, a `Transcript`, a `RunSummary`), but never on the layers *above*
  them (`commands/`), and the domain never depends back on them. The litmus test
  (`rust-engineering-architecture-guide.md` Part 2): `inference/` would still compile
  with `persistence/`, `metrics/`, and `commands/` deleted.

Enforced by the guardrail target `backend/tests/layering_guard.rs` (see
[Robustness](#robustness)) — run on every PR: no file under `inference/` may contain
`use crate::commands`, `use crate::persistence`, or any `tauri::`; and neither
`persistence/` nor `metrics/` may contain `use crate::commands`. (The folder-size rule
lives in the separate `backend/tests/folder_taxonomy.rs` target — see
[Folder taxonomy](#folder-taxonomy).)

### Pattern 1 — Sink boundary (invert the dependency)

When the domain must emit progress/results, it defines a **trait** describing the
events in plain domain terms; the IPC layer implements that trait by emitting
Tauri events. The domain depends on its own trait, never on the IPC layer.

```
inference/compare/sink.rs   pub trait CompareSink { fn token(..); fn done(..); … }
commands/compare.rs         impl CompareSink for TauriCompareSink { … app.emit(…) }
```

This is why `commands/` can know about `inference/` types but not the reverse.
The eval **batch dispatcher** follows the same shape: `inference/eval/batch.rs`'s
`run_batch` runs a strict sequential model×task queue (never fans out local
inference → OOM-safe) and emits through a `BatchSink`; `commands/eval/batch_cmd.rs`
implements it as `TauriBatchSink`, streaming `batch-progress`/`agentic-step`/
`batch-complete` over one channel so the IPC boundary is crossed once. The runner
is generic over a `ModelTurn` seam (real `BackendTurn` vs a scripted model), so the
whole queue is unit-tested without HTTP. On the frontend, the matching consumer
(`batchStore`) buffers events and flushes to reactive state at ≤60Hz via
`requestAnimationFrame`, so a model's token firehose never triggers a per-event
render.

The batch command also carries the **Phase-9 run-shape parameters** end-to-end:
`run_batch_eval` takes `tier`/`decoyTools` alongside `k`/`maxSteps`, persists them on
`RunConfig` (`#[serde(default)]` so older resumable job logs still parse), and
`apply_overrides` stamps them onto each agentic spec at run time (tier → `spec.tier`
+ derived `pass_k_for(tier)` when no explicit `k`; decoys → `spec.axes.decoy_tools`).
The Tests page's tier-`Auto` mode + HW hint read a separate **`get_hardware_tier`**
command (`commands/eval/readiness_cmd.rs`) that classifies the machine via the
readiness engine's `classify_bytes` + `default_required_tier` — one source of truth
for the GB thresholds, never duplicated in TS.

On the **read** side (Phase 9B), the per-tier breakdown the Agent Report deep-dive
renders is computed once in `agg_agentic` (the enriched `TierStat` carries per-tier
`avg_steps` + `failures`) and surfaced on `ModelVerdict.by_tier`/`failures`. A single
`readiness::inputs::native_first_source` helper selects the native-first aggregate for
the gate, the per-tier breakdown, **and** the failure taxonomy, so the displayed numbers
can never come from a different pass than the verdict gated on.

### Pattern 2 — Thin command, pure core

A `#[tauri::command]` does three things only: validate input, wire Tauri
plumbing (build the sink/handler, manage `State`), and delegate to a pure
`*_inner` core. The core takes plain data + callbacks and is unit-testable
without a Tauri runtime.

Reference: `commands/prompt.rs` (thin) → `commands/prompt_run.rs::run_prompt_inner`
(pure, integration-tested with mockito). New commands follow this split; logic
that needs a test belongs in the core, not the command.

### The `gui` feature seam (headless CLI build)

The crate has one cargo feature seam: `gui` (default-on). The desktop app is
`cargo build` / `pnpm tauri dev` exactly as before; the headless `qm` CLI is
built with `--no-default-features`, which compiles OUT every Tauri dependency
(the release-binary path — no webkit/GTK on a server). The rules:

- GUI-only modules (`commands/` except the six the CLI reaches: `eval`, `mcp`,
  `models`, `prompt`, `remote`, `system`) are gated at the `mod` declaration
  with `#[cfg(feature = "gui")]`.
- Inside a shared command file, `#[tauri::command]` wrappers whose signatures
  carry Tauri types (`AppHandle`, `State`) are gated wholesale; wrappers with
  plain signatures that the CLI calls directly use
  `#[cfg_attr(feature = "gui", tauri::command)]` so the fn stays callable lean.
- Logic both faces need lives in the engine, not the command layer
  (`probe_native_tools` → `inference/eval/batch.rs`; the MCP task-spec wire
  types → `commands/mcp/task_cmd.rs`, the spec→ToolTask stage).
- The `quantamind` bin has `required-features = ["gui"]`, so the lean build
  skips it; `build.rs` runs `tauri_build` only when the feature is on.
- CI enforces the seam: the `qm-headless` job builds `--no-default-features`
  on a runner with no Tauri system libs and fails if the binary links
  gtk/webkit/asound.

Update this section when the set of layers or allowed edges change, or a new
cross-layer boundary needs a sink/callback contract.

---

## Robustness

**No silent failures, no leaky data.** Every failure is either handled or
surfaced. The user (or a test, or a log) must be able to tell that something
went wrong. Fabricating a plausible-looking result is worse than an error,
because it hides.

### No silent failures

- **No `let _ =` on a fallible call** (a `Result`, a `JoinHandle`) unless it is a
  documented best-effort cleanup — and even then route it through a helper that
  logs the failure. For Tauri event emission use the `log_emit` helper, never a
  bare `let _ = app.emit(...)`: a dropped event silently freezes the UI.
- **Don't swallow serialization errors.** `serde_json::to_value(...)` and friends
  must log (or propagate) on failure, not vanish in an `if let Ok(_)`.
- **Observe spawned tasks.** Don't `let _ = join_all(handles)`; inspect each
  result and surface a panic/error as an event, not nothing.
- **Frontend: validation failures surface to state.** When a zod `safeParse`
  fails on an IPC payload, set an error state on the affected row/download (and
  log) — never `console.error` then `return`, which leaves the UI frozen. Promise
  rejections get a real handler, not a bare `.catch(() => {})`.
- **Frontend: render throws are bounded per panel.** Every top-level tab is
  always-mounted (`App` renders them all behind `hidden=`), so an unbounded render
  throw in any one would unmount the whole app. Each view is wrapped in
  `ErrorBoundary` (co-located with the `ErrorCard` it renders, in `shared/ui/ErrorCard`),
  which **logs** the error + component stack (never just swallows it into the card) and
  offers a Retry that remounts the subtree.

### No leaky data

- **Never fabricate data on error.** No zero-on-poison: a `token_count: 0` after a
  panic is indistinguishable from a real empty run. Emit a distinct
  degraded/error signal instead, so the UI can show "incomplete," not "done."
- **Don't blank error context.** `resp.text().await.unwrap_or_default()` turns an
  HTTP error body into "" — keep it (or annotate the read failure) so diagnostics
  survive.
- **Validate at every boundary.** zod on inbound IPC payloads (TS), `validator` +
  serde on inbound commands (Rust). Untrusted data never reaches domain logic
  unchecked.

### Independent panels degrade independently

A read that aggregates two independent sources must not fail wholesale when one
is down. `get_disk_usage` reports filesystem free/total (from `sysinfo`) plus a
model-bytes sum (from llama.cpp `/v1/models`). llama.cpp being unreachable zeroes only
the model sum (`disk_usage_for`) — it never fails the whole call, which used to
surface "the server is unreachable" inside the *Storage* panel. The zeroed sum is
not a leaky "done" signal: the llama.cpp-down state is shown distinctly by the
status bar and the installed-models list, so the user is never misled.

`clear_app_cache` (Downloads → **Clear cache**) deletes only regenerable caches
under `app_config_dir` via an explicit allow-list (`jobs/`, `history/`,
`batch_reports/`, `traces/`, `cliff/`, `recent_workspaces.yaml`) and returns the
measured bytes freed. Downloaded models, custom eval collections (`evals/`),
readiness profiles (`readiness/`), and settings are absent from the list, so a
clear can never destroy them. Logic lives in a pure `clear_cache_in(base)` core
(unit-tested over a tempdir); the thin command only resolves the config dir.

### Errors are typed

Rust returns `Result<T, AppError>`; TS returns discriminated unions over IPC, not
thrown errors. **No `unwrap()`/`expect()`/`.parent().unwrap()` outside tests** —
prove the invariant or return a typed error.

> Known limitation / future option: `AppError` variants are stringly-typed
> (`Inference(String)`), so io errno / HTTP status is flattened to a message.
> Enriching them is high-ripple and deferred; the discriminated-union-over-IPC
> shape is acceptable for now.

### Guardrail

`backend/tests/layering_guard.rs` enforces the one-way dependency law: under
`inference/`, no `use crate::commands`, no `use crate::persistence`, and no `tauri::`;
and neither `persistence/` nor `metrics/` may `use crate::commands` (see
[The dependency law](#the-dependency-law)). The folder-size rule is a separate target,
`backend/tests/folder_taxonomy.rs` (see [Folder taxonomy](#folder-taxonomy)).

Update this section when a new class of failure or boundary appears, or the
error model changes (e.g. structured `AppError`).

---

## Folder taxonomy

One concern per file (see [Conventions](process.md#conventions)); and **no folder holds
more than 10 files**. When a folder reaches the limit, split it into sub-folders
grouped by concern — never a `misc/`/`utils/` catch-all. Finding a file should be
a matter of guessing the right concern folder.

Enforced by a guardrail test on each side (`backend/tests/folder_taxonomy.rs`,
`frontend/src/__tests__/folderTaxonomy.test.ts`). `__tests__` dirs are exempt —
they mirror their source one-to-one, so their size is already bounded.

> **Known debt (measured 2026-08-02):** six backend folders are over the limit —
> `persistence/` (13), `inference/eval/agentic/v2/` (13), `inference/mcp/` (11),
> `inference/eval/toolcall/` (11), `commands/llama/` (11), and `backend/src/`
> itself (11). The previous note here listed four and named `commands/vllm/`,
> which no longer exists; run `cargo test --test folder_taxonomy` for the current
> set rather than trusting this list. The taxonomy test is kept in its **own** target
> (`folder_taxonomy.rs`), separate from the dependency-law target
> (`layering_guard.rs`), so the law can gate CI while this split is worked off as a
> dedicated refactor. Tracked in `docs/restructure-todo.md`.

### Target sub-folder layout

These four folders exceeded the limit and are split as follows (the reorg lands
one folder per commit, behavior unchanged).

- **backend `commands/`** (was 36 files): `prompt/` · `compare/` · `models/` ·
  `hf/` · `gguf/` · `llama_cpp/` · `workspace/` · `storage/` · `settings/` ·
  `system/` (health, feasibility, hardware, onboarding)
- **backend `inference/`** (was 33 files): `llama_cpp/` · `llama/` · `vllm/`
  (wire + chunk + stats + stream + backend, plus `vllm/server/` =
  runtime/locate/stderr/endpoint for the launcher) · `gguf/` · `hf/` · `pull/` ·
  `create/` · `compare/` · `eval/` (deterministic mini-eval task + scoring, plus
  `eval/toolcall/` — prompt-based, single-turn, structural tool-call eval) ·
  `http/` (http + ndjson) · `backend/` (trait + kind) · `generate/` (spec +
  options) · `chat/` (templates) · `vram_math.rs` (canonical f16 KV-cache formula,
  unit-tested). `llama_cpp/` also has `llama_cpp_show.rs` — the Tauri-free the GGUF header client
  (template, capabilities, raw `model_info`) behind `commands/models/model_inspect.rs`
  (which also parses `ModelInspect.dims` + exposes `estimate_kv_cache_bytes`); frontend IPC
  in `shared/ipc/system/inspect.ts`. The KV-aware VRAM fit lives in
  `shared/memory/useVramFit.ts` + `features/models/fit.ts::fitOfNeed` (the Quant tab that
  first hosted it was removed; ParamsControl and the Context Stress Test still consume the hook);
  the curated memory-bandwidth lookup is in `commands/system/hardware_mem.rs`. The 5.12–5.15
  diagnostics are mostly frontend over data already fetched:
  `features/eval/CpuFallbackBanner` (silent CPU fallback, from the server's status endpoint), `features/inspector/ContextBudgetBar`
  (prompt_eval_count / context_length), and the Context Stress Test (`features/eval/cliff.ts` +
  `useContextCliff` + `ContextCliffChart`, visx). Built-in eval presets (curated + `tasks_finance.json`)
  are enumerated by `toolcall/tasks.rs::BUILTIN_COLLECTIONS` behind `list_builtin_collections` /
  `get_builtin_collection`.
- **backend `inference/eval/agentic/`** (was 11 files): the run-judgment concern
  splits into `agentic/scoring/` (`report.rs` = `AgenticReport`/`FailureTracker`/
  `FailureKind`/`RunOutcome`/`TopError`; `endstate.rs` = `checkpoint_matches` /
  `validate_call`). The run loop (`runner`, `model_turn`, `context`, `step`),
  task definition (`spec`, `sandbox`, `build`) stay at the root. **`agentic/v2/`** is
  the Phase 9-v2 authored-scenario engine: `collection`/`transpile` (load a v2 JSON
  object → engine `ToolTask`s), `match` (wildcard + `must_not_call`), `world_state`
  (ground-truth responder), `scenarios` (the bundled collections via `include_str!`,
  under `v2/scenarios/`, plus `CURATED_IDS`/`is_curated` — the 12 the pickers offer,
  three per tier; the rest stay loadable-by-id engine fixtures), and `generator` (per-run procedural
  instancing — seeded entity-id remap). v2 runs on the SAME runner — no second
  execution path (`run_agentic_with` drives Pass^k via a per-run sandbox factory).
- **frontend `features/workspace/components/`** (was 17 files): `model-select/` ·
  `prompt/` (editor + params) · `run/` (single/multi + controls + output) ·
  `status/` (status bar, llama_cpp control, errors)
- **frontend `shared/ipc/`** (was 26 files), grouped by domain: `core/` (client,
  error, errorInfo, timeout, types) · `events/` (event names + payload zod
  schemas) · `compare/` · `models/` · `workspace/` · `settings/` · `system/` ·
  `eval/` (`evals`, `toolcall`, `registry` — the custom-eval CRUD + ToolTask zod)
- **custom-eval registry** spans the layers by responsibility: the storage-free
  runner takes a `Vec<ToolTask>`; `persistence/evals.rs` owns file I/O;
  `commands/eval/eval_registry.rs` is the thin CRUD + path-only import; UI lives
  in `features/eval/` (`useEvalRegistryStore`, whose `NEW_COLLECTION` sentinel /
  `startNew` model the unsaved-new selection). The manager UI is a master-detail
  split under `components/manager/` (`EvalManager` orchestrator + `NameDialog`,
  `TaskListView`, `TaskDetailView`, `StatsBar`) — kept in a subfolder so
  `components/` stays ≤10 files — over feature-root modules `evalDraft.ts` (draft
  shape + Save/Run validation) and `verdict.ts` (pass/fail + score helpers, shared
  with `ToolCallPanel`).
- **collection matrix & history** follows the same layering: pure aggregation in
  `inference/eval/toolcall/matrix.rs` (`build_matrix`/`summaries`, no async/I/O);
  the append-only, 100-entry-capped log in `persistence/eval_history.rs`; the thin
  sequential runner + history write in `commands/eval/matrix_cmd.rs`
  (`run_collection_matrix`/`load_collection_history`). UI is a separate
  `components/matrix/` subfolder (`MatrixPanel` + `MatrixGrid`, `HistoryTimeline`,
  `ModelToggles`) mounted in `EvalPage`, over `shared/ipc/eval/matrix.ts`.
- **pipeline visualizer** reuses the runner's single-task path: `eval.rs` exposes
  `trace_one` (+ `TraceResult` = system message + raw output + verdict), which
  `run_eval` loops over and the `trace_toolcall_task` command calls directly — so
  the trace matches a real run. UI is a `components/pipeline/` subfolder
  (`PipelinePanel` + `ConfigPhase`, `SystemMessagePhase`, `StreamPhase`,
  `VerifyPhase`) over `traceToolcallTask` in `shared/ipc/eval/toolcall.ts`.
- **trace cache** keeps a run's per-task traces so a drill-down never re-runs
  inference: `run_eval_traced` (in `eval.rs`) returns the full `TaskTrace`s
  alongside the report, and both runners (`run_toolcall_eval` Simulator,
  `run_collection_matrix`) cache them best-effort into the `traces/` managed dir
  via `persistence/eval_trace_store.rs` (one JSON file per collection, models
  keyed within, upsert by task id, 1 MB read guard). `load_toolcall_trace`
  serves a cached `(collection, model, task)` trace; `PipelinePanel` loads it on
  a `View Trace` / Matrix-cell handoff (▶ still re-runs live). A cache miss/write
  failure degrades gracefully to a live run — never blocks the eval.

### Rules for a split

- Move files only; do not change behavior in a reorg commit.
- Update the module's `mod.rs` (Rust) / import paths (TS); run the full suite
  green before committing.
- Keep tests beside their code through the move.

Update this section when a folder crosses 10 files and needs a new sub-grouping,
or a sub-folder's concern boundary changes.

---

## Folder structure

```
QM-Dev/
├── .github/
│   ├── workflows/{ci.yml,release.yml,nightly.yml}
│   └── PULL_REQUEST_TEMPLATE.md
│
├── frontend/                       # React + TS + Vite app
│   ├── src/
│   │   ├── app/{App.tsx,routes.tsx,providers.tsx}
│   │   ├── features/
│   │   │   ├── workspace/          # Phase 1
│   │   │   │   ├── components/{PromptEditor,OutputStream,ModelPicker,RunControls}.tsx
│   │   │   │   ├── hooks/{useStreamingRun,usePromptStore}.ts
│   │   │   │   ├── state/workspaceStore.ts
│   │   │   │   ├── types.ts
│   │   │   │   ├── schemas.ts      # zod
│   │   │   │   └── __tests__/
│   │   │   ├── inspector/          # Phase 4
│   │   │   ├── bench/              # Phase 3
│   │   │   └── settings/           # Phase 2
│   │   ├── shared/
│   │   │   ├── components/
│   │   │   ├── ipc/{client.ts,types.ts,__tests__/}
│   │   │   └── styles/tokens.css
│   │   ├── test/setup.ts
│   │   ├── main.tsx
│   │   └── index.css
│   ├── index.html
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── tsconfig.json / tsconfig.node.json
│   ├── vite.config.ts / vitest.config.ts
│   ├── tailwind.config.js
│   └── postcss.config.js
│
├── backend/                        # Rust + Tauri 2 app
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── commands/{mod,prompt,models,settings,workspace}.rs
│   │   ├── inference/{backend,llama,vllm,openai,chat,generate}/
│   │   ├── metrics/{mod,timing,vram}.rs
│   │   ├── persistence/{mod,prompts,history}.rs
│   │   ├── validation/{mod,schemas}.rs
│   │   └── errors.rs
│   ├── tests/{llama_cpp_stream,models_list,prompt_stream}.rs
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── capabilities/
│   └── icons/
│
├── docs/                           # this directory
├── CLAUDE.md .gitignore
└── LICENSE README.md CHANGELOG.md
```

### Rationale

- **`frontend/` + `backend/` top split.** Two languages, two toolchains.
  Co-locating each side's configs with its source means a frontend dev rarely
  needs to read backend files and vice versa.
- **`features/` over `components/` at top level.** Each feature is a vertical
  slice: components + hooks + state + tests. Deletable in one `rm -rf`.
- **`commands/` mirrors `features/`.** Every command corresponds to a frontend
  need. If they drift, something is wrong.
- **`__tests__/` next to code.** Rust integration tests are the exception —
  they live in `backend/tests/` because cargo requires it.

### Tauri CLI: pointing at `backend/`

Tauri 2's CLI discovers the project by searching subfolders of cwd for
`tauri.conf.json`. From `frontend/` it can't see `backend/`, so
`frontend/package.json`'s `tauri` script is `"cd .. && tauri"` — shifting cwd to
the QM-Dev root where `backend/` is a subfolder. `backend/tauri.conf.json` then
references the frontend via `pnpm --dir=../frontend dev` / `build` and
`frontendDist: ../frontend/dist`. Both directions of the hop are explicit.

New work almost never adds a top-level folder; it fits into a new feature
(`frontend/src/features/<name>/`) or a new command + domain module.

### The headless `qm` CLI: `backend/src/bin/qm/`

The Tauri app is the default bin (`backend/src/main.rs`). The **`qm` CLI** is a second
`[[bin]]` on the *same* crate (ADR 0001 — one crate, no workspace), at
`backend/src/bin/qm/main.rs`. It is deliberately thin: parse args, call a pure engine in
`quantamind_lib`, render, map the exit code — no logic in the bin. All CLI command engines live under
**`backend/src/cli/`** — `cli/doctor/`, `cli/run/`, `cli/init/` — composing the existing per-backend
health/credential/capability probes rather than re-implementing them. Where a command has genuinely
per-engine logic it's split per engine: `cli/doctor/probe/{llama_cpp,openai_local,remote}` (the three
distinct reachability strategies). Reference: [docs/cli/README.md](cli/README.md).


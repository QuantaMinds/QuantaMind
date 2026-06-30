# QuantaMind — Codebase Documentation

> A native desktop workbench for running, comparing, and managing local large
> language models. Tauri 2.x + Rust backend + React 18 / TypeScript frontend.
> Local-first, no telemetry, ~30 MB binary.

This folder is the **deep, file-by-file reference** for the entire codebase.
Every page answers three questions for each module, folder, and file:

- **Why** it exists (the problem it solves / the rule it enforces)
- **What** it is (the types, functions, exports, contracts)
- **How / Where** it is used (who calls it, which feature/tab/page it powers)

For the higher-level narrative (engineering principles, install pipeline,
feature tours) see the root [`README.md`](../../README.md). For the binding
rules see [`CLAUDE.md`](../../CLAUDE.md) and the three engineering docs:
[`architecture.md`](../architecture.md), [`process.md`](../process.md),
[`reference.md`](../reference.md).

---

## How to read this folder

The code splits cleanly into two halves connected by **Tauri IPC** (the
frontend calls Rust `#[tauri::command]` functions through a typed client; Rust
streams events back). Start with the two overview pages, then jump to the
subsystem you care about.

```
React UI  ──(invoke "command_name", args)──▶  Rust #[tauri::command]
   ▲                                                  │
   └──────────(emit "event_name", payload)◀───────────┘
```

### Backend (Rust — `backend/src/`)

| Page | Covers | Source roots |
|---|---|---|
| [backend-overview.md](backend-overview.md) | Entry points, command registration, IPC contract, app lifecycle, errors, sync, time, validation, metrics | `lib.rs`, `main.rs`, `commands/mod.rs`, `commands/app_lifecycle.rs`, `commands/emit.rs`, `errors.rs`, `sync.rs`, `time_iso.rs`, `validation/`, `metrics/` |
| [backend-inference-backends.md](backend-inference-backends.md) | The `InferenceBackend` trait, HTTP/NDJSON plumbing, and the three engines | `inference/backend/`, `inference/http/`, `inference/llama/`, `inference/mlx/`, `inference/ollama/`, `inference/generate/`, `inference/token_handler.rs`, `inference/chat/` |
| [backend-models-hf-gguf.md](backend-models-hf-gguf.md) | Model listing/inspection/pull, Hugging Face browse+install, GGUF parsing, Modelfile create, VRAM math | `commands/models/`, `commands/hf/`, `commands/gguf/`, `inference/hf/`, `inference/gguf/`, `inference/pull/`, `inference/create/`, `inference/vram_math.rs` |
| [backend-eval-engine.md](backend-eval-engine.md) | Tool-calling eval, agentic runner, Context Stress Test, readiness/VRAM-fit, batch queue, scoring | `commands/eval/`, `inference/eval/` |
| [backend-stt.md](backend-stt.md) | whisper.cpp sidecar, audio capture/decode, transcription, STT profiling, STT eval | `commands/stt/`, `commands/audio/`, `inference/stt/` |
| [backend-compare.md](backend-compare.md) | Side-by-side multi-model runner, memory-fit gate, export | `commands/compare/`, `inference/compare/` |
| [backend-prompt-workspace-system.md](backend-prompt-workspace-system.md) | Single-prompt run, prompt templates, workspaces, history, settings, storage, system/hardware | `commands/prompt/`, `commands/prompt_templates/`, `commands/workspace/`, `commands/settings/`, `commands/storage/`, `commands/system/` |
| [backend-persistence.md](backend-persistence.md) | On-disk stores: prompts, evals, history, jobs, readiness, publish, STT, settings, workspaces | `persistence/` |
| [backend-publish.md](backend-publish.md) | Publish-to-board flow, OAuth/PKCE identity, canonical payload, validation | `commands/publish/`, `persistence/publish/` |

### Frontend (TypeScript/React — `frontend/src/`)

| Page | Covers | Source roots |
|---|---|---|
| [frontend-overview.md](frontend-overview.md) | App shell, top-nav tabs, header/global controls, hotkeys, the typed IPC layer, shared state stores, shared UI kit | `App.tsx`, `AppHeader.tsx`, `GlobalControls.tsx`, `main.tsx`, `appHotkeys.ts`, `shared/` |
| [frontend-workspace.md](frontend-workspace.md) | Workspace tab (prompt editor, model select, run/stream, server controls) + workspace file tree | `features/workspace/`, `features/workspaces/` |
| [frontend-compare-analysis.md](frontend-compare-analysis.md) | Analysis tab (compare columns, diff, metrics chart, export) | `features/compare/` |
| [frontend-eval.md](frontend-eval.md) | Tests tab (manager, matrix, pipeline, scoreboard, trajectory, Context Stress Test, tool-call) | `features/eval/` |
| [frontend-models.md](frontend-models.md) | Models + Downloads tabs (HF/Ollama/local install, cards, storage) | `features/models/` |
| [frontend-stt.md](frontend-stt.md) | Voice/STT (header control, catalog, server panel, workspace transcribe, STT eval, STT inspector) | `features/stt/`, `features/sttWorkspace/`, `features/sttEval/`, `features/sttInspector/` |
| [frontend-inspector-quant-agentreport.md](frontend-inspector-quant-agentreport.md) | Latency tab, Quant sub-tab, Agent Report tab | `features/inspector/`, `features/quant/`, `features/agentReport/` |
| [frontend-support-features.md](frontend-support-features.md) | Settings, Onboarding, Help/Updater, Feedback, History, Audit | `features/settings/`, `features/onboarding/`, `features/help/`, `features/feedback/`, `features/history/`, `features/audit/` |

---

## The top-level navigation (what the user sees)

`frontend/src/App.tsx` renders ten top-nav tabs (`TABS` array). Each maps to one
page component and one documentation page above:

| Tab (`TopView` id) | Page component | Doc |
|---|---|---|
| `workspace` | `features/workspace/components/Workspace.tsx` | [frontend-workspace](frontend-workspace.md) |
| `compare` ("Analysis") | `features/compare/components/AnalysisPage.tsx` | [frontend-compare-analysis](frontend-compare-analysis.md) |
| `inspector` | `features/inspector/components/InspectorPage.tsx` | [frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md) |
| `eval` | `features/eval/components/EvalPage.tsx` | [frontend-eval](frontend-eval.md) |
| `audit` | `features/audit/components/AuditPage.tsx` | [frontend-support-features](frontend-support-features.md) |
| `agentReport` | `features/agentReport/components/AgentReportPage.tsx` | [frontend-inspector-quant-agentreport](frontend-inspector-quant-agentreport.md) |
| `models` | `features/models/components/ModelsPage.tsx` | [frontend-models](frontend-models.md) |
| `downloads` | `features/models/components/DownloadsPage.tsx` | [frontend-models](frontend-models.md) |
| `settings` | `features/settings/components/SettingsPage.tsx` | [frontend-support-features](frontend-support-features.md) |
| `help` | `features/help/components/HelpPage.tsx` | [frontend-support-features](frontend-support-features.md) |

The Quant view is a **sub-tab merged into Analysis** (see commit history); it is
documented alongside Latency/Agent Report because it shares the readiness
data model.

---

## The backend command surface (the IPC contract)

`backend/src/lib.rs` is the single registration point. Its `tauri::Builder`:

1. Registers Tauri plugins (`dialog`, `process`, `shell`, `updater`).
2. `.manage(...)`s 17 pieces of shared mutable state (run cancellation tokens,
   install trackers, server handles, auth state).
3. `.setup(...)` reclaims orphaned sidecar servers, installs a signal reaper,
   reconciles half-installed STT artifacts, and clears recording scratch.
4. `.on_window_event(...)` reaps sidecars + `app.exit(0)` on `CloseRequested` —
   on macOS closing the window doesn't quit a Tauri app, so without this the
   spawned Ollama would linger after "closing" the app.
5. `.invoke_handler(generate_handler![...])` exposes ~120 commands.
6. `.run(reap_on_exit)` guarantees child servers die with the app (Cmd+Q path);
   the signal reaper covers SIGINT/SIGTERM. Three exit paths, one idempotent reap.

Every command in that list is documented on the backend page for its subsystem.
The managed-state structs and the lifecycle hooks are documented in
[backend-overview.md](backend-overview.md).

---

## Conventions used in these docs

- **File** headers are repo-relative paths, clickable as `path:line` where a
  specific line is cited.
- Code snippets are **real excerpts** (possibly trimmed with `…`) — they show
  the actual signature/shape, not pseudocode.
- "Sink" = a thin object that owns side effects (event emit, disk write) so the
  pure runner stays testable. See [architecture.md#layering](../architecture.md).
- Test files (`*_tests.rs`, `*.test.tsx`) are noted next to the unit they cover
  rather than given their own sections.
```

# ARCHITECTURE

The five-minute map of QuantaMind. Read this before the territory. For the deep
dive see [`docs/architecture.md`](docs/architecture.md); for the philosophy behind
the shape see [`rust-engineering-architecture-guide.md`](rust-engineering-architecture-guide.md).

QuantaMind is a **Tauri 2 desktop app**: a React/TypeScript frontend in a webview
talks to a **Rust backend** over JSON IPC; the backend calls a local llama.cpp server
and bundled sidecars (`llama-server`, `vllm_lm.server`) over HTTP.

## The shape: hexagonal, one crate

The backend is **Ports & Adapters (hexagonal)** expressed as *modules within a single
crate* — the stage the [guide](rust-engineering-architecture-guide.md) endorses until
team size or build time forces a workspace split (see
[ADR-0001](docs/adr/0001-single-crate-not-workspace.md)). The domain sits at the
center; the outside world (IPC, HTTP, the filesystem) is reached only through traits.

```
            ┌──────────────────────────────────────────────────────┐
            │  COMPOSITION ROOT — backend/src/lib.rs (run())        │
            │  registers plugins, .manage()s state, wires commands  │
            │   ┌────────────────────────────────────────────────┐ │
            │   │  DRIVING ADAPTER — commands/                    │ │
            │   │  #[tauri::command] fns: deserialize, call a     │ │
            │   │  use case, map AppError, emit events. Thin.     │ │
            │   │   ┌──────────────────────────────────────────┐  │ │
            │   │   │  DOMAIN — inference/                      │  │ │
            │   │   │  eval scoring, pass^k, readiness verdict, │  │ │
            │   │   │  cliff probe, ResponderKind/BackendKind   │  │ │
            │   │   │  enums, GGUF parsing. PURE. Tauri-free.   │  │ │
            │   │   │  Defines PORTS (traits).                  │  │ │
            │   │   └──────────────────────────────────────────┘  │ │
            │   │   DRIVEN ADAPTERS (implement the ports):        │ │
            │   │   • inference/{llama_cpp,llama,vllm}/*_backend.rs   │ │
            │   │     → the InferenceBackend trait (HTTP clients) │ │
            │   │   • persistence/  → filesystem repos            │ │
            │   │   • metrics/      → timing/throughput           │ │
            │   └────────────────────────────────────────────────┘ │
            └──────────────────────────────────────────────────────┘
                     source dependencies point INWARD ──►
```

## The Dependency Rule (the one law)

> **Source dependencies point inward. The domain depends on nothing above it;
> everything depends on the domain.**

Concretely, edges go one way only:

```
commands/  ──►  inference/ (domain)  ◄──  persistence/ , metrics/
  (IPC,            pure core,              driven adapters that
  driving)         defines ports          depend on domain types
```

- **`commands/` is the only layer that names `tauri::`** (`AppHandle`, `State`,
  `Emitter`, `#[tauri::command]`). It is the *driving adapter* / "controller" ring.
- **`inference/` is the domain and must be Tauri-free.** It must not
  `use crate::commands`, must not name any `tauri::` type, and must not
  `use crate::persistence`. When domain code must report progress it takes a **sink
  trait** (a port) — never an `AppHandle`.
- **`persistence/` and `metrics/` are driven adapters.** They legitimately depend on
  domain *types* (to serialize a `ReadinessVerdict`, a `Transcript`, a run summary),
  but the domain never depends back on them.

**Litmus test** (guide Part 2): the domain should compile with the HTTP, IPC, and
filesystem code deleted. This invariant is mechanically enforced by
[`backend/tests/layering_guard.rs`](backend/tests/layering_guard.rs) and run in CI.

## Ports & adapters in this repo

| Port (trait, in the domain) | Driven adapters (implementations) |
|---|---|
| `InferenceBackend` (`inference/backend/backend.rs`) | `llama.cppBackend`, `LlamaCppBackend`, `VLlmBackend` — selected by the `BackendKind` enum, not `dyn`. |
| `CompareSink` / `BatchSink` | `commands/` implement them as Tauri sinks that `emit` IPC events. This **inverts** the domain→IPC dependency. |

Backends are chosen by matching a closed `BackendKind` enum, so adding one makes the
compiler flag every seam — see
[ADR-0002](docs/adr/0002-enums-not-strings.md).

## Errors

One serializable `AppError` enum (`backend/src/errors.rs`, `thiserror`) is the return
type of every command, mapped to the IPC boundary via `#[serde(tag, content)]`. The
domain uses typed results and never fabricates a value to paper over absence — missing
metrics are `Option`/`N/A`, never a placeholder `0` (see
[ADR-0005](docs/adr/0005-no-fabricated-metrics.md)).

## Where things live

| Path | Ring | What |
|---|---|---|
| `backend/src/lib.rs` | composition root | builds the Tauri app, wires everything |
| `backend/src/commands/` | driving adapter | ~123 `#[tauri::command]`s, thin |
| `backend/src/inference/` | domain + driven HTTP adapters | scoring, readiness, eval, backends |
| `backend/src/persistence/` | driven adapter | JSON/YAML/JSONL on disk |
| `backend/src/metrics/` | driven adapter | TTFT, tokens/sec, timeline |
| `backend/src/errors.rs` | crosses layers | `AppError`, mapped at the edge |
| `frontend/src/` | client app | React features + `shared/ipc/` (only IPC caller) |

## Conventions

- One concept per file; **no folder holds more than 10 `.rs` files** — split by
  *concern*, never a `utils/` drawer (`docs/architecture.md#folder-taxonomy`).
- Decisions that aren't obvious from the code get an ADR in
  [`docs/adr/`](docs/adr/).
- The full subsystem walkthroughs live in [`docs/codebase/`](docs/codebase/).

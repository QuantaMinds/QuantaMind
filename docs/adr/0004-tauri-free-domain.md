# 0004 — `inference/` is Tauri-free; domain→IPC progress crosses via sink ports

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Long-running domain work (a compare run, a batch eval, a transcription) must stream
progress to the React frontend. The naive wiring passes a Tauri `AppHandle` into the
engine so it can `emit` events — which couples the pure domain to the IPC framework,
makes it untestable without Tauri, and inverts the Dependency Rule.

## Decision

The domain (`inference/`) is **Tauri-free**: it never names a `tauri::` type and never
`use crate::commands`. When it must report progress it depends on a **sink trait** it
defines in its own terms — `CompareSink`, `BatchSink`, `TranscribeSink`. The IPC layer
(`commands/`) implements that trait (e.g. `TauriBatchSink`) and does the `emit`. The
dependency points inward: `commands` knows `inference`, never the reverse.

## Consequences

- The engine is unit-tested by passing a recording/in-memory sink — no webview, no HTTP.
- The IPC boundary is crossed in exactly one place per feature, easy to audit.
- Enforced mechanically by [`backend/tests/layering_guard.rs`](../../backend/tests/layering_guard.rs)
  (no `use crate::commands` under `inference/`, and — after the 2026-06 hardening — no
  `tauri::` either).

## Alternatives considered

- **Pass `AppHandle` into the engine:** less boilerplate, but couples domain to framework
  and breaks testability — rejected. Sink ports are the guide's Part 4b/8.1 pattern.

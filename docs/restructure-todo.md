# Architecture Hardening — Task Tracker

Living checklist for aligning the backend to
[`rust-engineering-architecture-guide.md`](../rust-engineering-architecture-guide.md)
**without** a workspace split and **without** adding crates (locked stack). Scope decided
with the maintainer: *harden the single crate now, scope the workspace split + new test
libs as rationale-backed future work.*

Per `CLAUDE.md`/`docs/process.md#workflow`, each phase is one commit:
**implement → test → inspect output → update docs → commit → next.**

**Test commands** (cargo is not on PATH):
- Green suite: `~/.cargo/bin/cargo test --lib`
- Guards: `~/.cargo/bin/cargo test --test layering_guard`
- Full `cargo test` is blocked by a pre-existing broken non-lib target — do not run it.

## Baseline (Phase 0, branch `refactor/architecture-hardening` off `main`)

- `cargo test --lib`: **899 passed, 0 failed, 17 ignored** ✅
- `cargo clippy --lib`: **49 warnings** (14 auto-fixable). Relevant to Phase 5.

## Phases

- [x] **Phase 0** — Baseline, branch, this tracker.
- [x] **Phase 1** — Root `ARCHITECTURE.md`; link from README; commit the guide file.
- [x] **Phase 2** — `docs/adr/` (README + `0000-template.md`) + backfilled ADRs 0001–0005.
- [x] **Phase 3** — `RunSummary` struct moved to `inference/eval/run_summary.rs`;
      `persistence/eval_history` now imports it; `inference` has zero `use crate::persistence`.
      *Verified:* `cargo test --lib` = 899 passed (unchanged); clippy 49 warnings (unchanged);
      history round-trip/back-compat/truncate tests green. Pure type relocation — serde shape
      byte-identical, so no behavioral surface for a live run to add over the round-trip tests.
- [ ] **Phase 4** — Correct `docs/architecture.md#layering` (domain-center) + extend
      `layering_guard.rs` (inference ⊁ `crate::persistence`/`tauri::`; persistence+metrics ⊁ `crate::commands`).
      *Gate:* `cargo test --test layering_guard` green; a temporary reverse `use` goes RED.
- [ ] **Phase 5** — `#![deny(unsafe_code)]` in `lib.rs`; CI gate (`fmt --check`,
      `clippy`, `cargo test --test layering_guard`).
      *Gate:* new CI steps pass locally. (Decide `-D warnings` vs informational given the 49 baseline.)
- [ ] **Phase 6** — `docs/process.md#future-considerations`: workspace-split triggers +
      `proptest`/`insta` proposals; ADR 0006.
      *Gate:* docs render; `git diff backend/Cargo.toml` shows no new deps.

## Out of scope (separate, later-approved plan)

- Full Cargo workspace split (per-layer crates).
- Actually adding `proptest`/`insta`/`mockall`/`nextest`/`async-trait`/`cargo-deny`.
- Speculative redesigns (collapse command subdirs, unify `AppState`, repo traits).

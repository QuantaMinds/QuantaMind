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
- [x] **Phase 4** — Corrected `docs/architecture.md#layering` to domain-center; extended
      the guard (inference ⊁ `crate::persistence`/`tauri::`; persistence+metrics ⊁
      `crate::commands`). Split the test file: **dependency law** →
      `tests/layering_guard.rs` (4 tests, GREEN, CI-gateable); **folder-size rule** →
      `tests/folder_taxonomy.rs` (isolated; RED on 4 pre-existing over-limit folders).
      *Verified:* `cargo test --test layering_guard` green; planted reverse `use` →
      RED, removed → green (teeth confirmed).
      ⚠️ **Follow-up debt:** split `persistence/`(12), `inference/eval/toolcall/`(11),
      `commands/vllm/`(11), `commands/llama/`(11) into concern sub-folders to green the
      taxonomy target — a dedicated refactor (out of this plan's scope per the
      "separate refactor commit" rule).
- [x] **Phase 5** — `#![deny(unsafe_code)]` added to `lib.rs` (compiles clean — no
      backend `unsafe`). CI gains `cargo test --test layering_guard` (the law) and
      `cargo clippy --lib` (error gate). *Verified locally:* lib 899 ✓, guard 4/4 ✓,
      clippy exit 0 ✓.
      ⚠️ **Deferred (would red-wash CI):** `fmt --check` (code is not rustfmt-clean;
      "rustfmt not enforced") and `clippy -D warnings` (49 pre-existing warnings). Both
      need a dedicated cleanup commit first — noted in `#future-considerations`.
- [x] **Phase 6** — `docs/process.md#future-considerations` gains the workspace-split
      (with activation triggers) and the test-lib/lint-strictness proposals; ADR 0006
      records them. *Verified:* `git diff main -- backend/Cargo.toml` empty (no new deps);
      lib 899 green.

## Status: all six phases complete ✅

Remaining tracked debt (separate, future PRs — see `#future-considerations` + ADR-0001/0006):
folder-taxonomy split (4 folders), `fmt --check` after a reformat, `clippy -D warnings`
after the 49 are cleared, the Cargo workspace split, and `proptest`/`insta` adoption.

## Out of scope (separate, later-approved plan)

- Full Cargo workspace split (per-layer crates).
- Actually adding `proptest`/`insta`/`mockall`/`nextest`/`async-trait`/`cargo-deny`.
- Speculative redesigns (collapse command subdirs, unify `AppState`, repo traits).

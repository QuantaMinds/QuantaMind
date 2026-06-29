# 0006 — Test libraries (`proptest`/`insta`) proposed and deferred

- **Status:** Proposed
- **Date:** 2026-06-28

## Context

The [Rust engineering guide](../../rust-engineering-architecture-guide.md) Part 6
recommends a richer test stack — `proptest` (property/round-trip), `insta` (snapshots),
`mockall`, `cargo-nextest` — and Part 7 a stricter lint gate (`clippy -D warnings`,
`fmt --check`). The project's **locked stack** (`../process.md#tech-stack`) mandates
`cargo test` + `mockito` and forbids adding crates without a rationale PR ("resist
additions; every dependency is maintenance debt"). The current suite (~899 lib tests) is
fast and green; clippy has ~49 pre-existing warnings; the tree is not rustfmt-clean.

## Decision

Do **not** add these crates or tighten the lint gate in the architecture-hardening
effort. Record them as scoped proposals in `process.md#future-considerations` for
separate, individually-approved PRs. Adopt in this priority order when taken up:
`proptest` (determinism seams) > `insta` (deterministic report snapshots) > `fmt --check`
(after one reformat commit) > `clippy -D warnings` (after the 49 warnings are cleared).
Leave `mockall`/`nextest`/`async-trait` out (hand-written fakes, sub-second suite, and
enum dispatch already cover their need).

## Consequences

- No stack churn now; the locked-stack discipline is honored.
- The value case is written down, so adoption is a small approved PR, not a debate.
- CI gates clippy as an *error* gate (not `-D warnings`) and runs the layering guard —
  real enforcement without red-washing on pre-existing warnings.

## Alternatives considered

- **Add `proptest`/`insta` now:** strong value, but bypasses the rationale-PR gate the
  locked stack requires — rejected for this effort, proposed for a follow-up.

# 0001 — Single crate with modules-as-layers, not a Cargo workspace

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The [Rust engineering guide](../../rust-engineering-architecture-guide.md) presents two
physical layouts for a hexagonal codebase: (a) a single crate with layers as modules,
and (b) a Cargo workspace with one crate per layer so the *compiler* enforces the
Dependency Rule. The backend is ~36k lines / ~353 files across a clean
`commands → inference → persistence/metrics` layering. A workspace split would touch
every file and the Tauri build, and stress the locked-stack discipline.

The guide's own guidance: *"Start simple (single crate, modules as layers). Evolve to a
workspace when team size or build times demand it."*

## Decision

Stay a single crate (`quantamind`) with layers expressed as modules. Enforce the
Dependency Rule with a **guardrail test** ([`backend/tests/layering_guard.rs`](../../backend/tests/layering_guard.rs))
plus the `≤10 files per folder` taxonomy, rather than with crate boundaries.

## Consequences

- Fast incremental builds, no workspace plumbing, one `Cargo.toml`.
- The dependency direction is enforced by a test, not the compiler — so the test must
  run in CI to have teeth (added in the same hardening effort).
- The split remains an option. Its concrete triggers are recorded in
  `docs/process.md#future-considerations`; revisit this ADR when one fires.

## Alternatives considered

- **Workspace now** (`engine-core`/`engine-app`/`backends`/`commands`): the textbook
  end state, but premature at current scale/team size — rejected until a trigger fires.

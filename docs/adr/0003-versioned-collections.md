# 0003 — An answer-key change mints a new versioned collection

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Eval collections (tool-call tasks, agentic scenarios) are content-hashed and their
results are published to a shared benchmark board. If the *answer key* of a collection
(expected calls, end-state, scoring) could be edited in place, two runs labelled with the
same collection id could have been scored against different truth — making published
numbers non-comparable and silently wrong.

## Decision

Treat a hashed collection as **immutable**. Any change to its answer key produces a
**new versioned collection** (new id/hash), never an edit of the existing one. Per-step,
local-only environment views are never published.

## Consequences

- A published row's collection id is a stable, reproducible contract: same id ⇒ same
  truth ⇒ comparable scores.
- Editing intent becomes explicit (a new version) instead of an invisible drift.
- Slightly more collections over time; acceptable for comparability.

## Alternatives considered

- **In-place edits:** simplest UX, but breaks cross-run comparability and reproducibility
  — rejected. This pairs with [ADR-0005](0005-no-fabricated-metrics.md) (honest metrics)
  and the publish-parity rule in `docs/architecture.md`.

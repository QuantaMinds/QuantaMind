# 0005 — Absence is `Option`/`N/A`, never a fabricated `0`

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The app reports measured numbers a user makes deployment decisions on: TTFT, tokens/sec,
RTF, confidence, KV-cache fit. When a backend doesn't report a value (no probabilities,
zero-span timing, an unavailable field), code is tempted to substitute a placeholder
`0`/empty string so a struct field is "filled." That placeholder is a *lie the type
system would have caught* — downstream code treats it as a real measurement.

## Decision

Make absence representable and propagate it. Optional measurements are `Option<T>` and
render as **"N/A"**, never a fabricated default. Pure math helpers return `None` on
degenerate input (e.g. `tokens_per_sec` returns `None` for zero span or zero count). The
silence-hallucination metric uses an *independent* non-ML VAD, never the model's own
opinion, so the number can't be circular.

## Consequences

- Reported metrics are either measured or honestly missing — never quietly faked.
- Callers must handle `None`, which is the point: the compiler forces the question.
- Pairs with [ADR-0003](0003-versioned-collections.md) (comparable, reproducible numbers)
  and the publish-parity rule in `docs/architecture.md`.

## Alternatives considered

- **Default to `0`/`""`:** simplest, but produces confident-looking wrong data on a tool
  whose whole value is honest measurement — rejected.

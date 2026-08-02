# 0002 — Closed discriminators are enums, not strings

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Several values in the engine are *closed sets*: the inference backend
(`BackendKind` = LlamaCpp | VLlm | VLlm), how the agentic sandbox answers a tool call
(`ResponderKind` = StaticMocks | WorldState | FileSystem | WebCorpus | WebUi), readiness
(`Verdict`), end-state rules, step kinds. These are determinism-critical: a wrong branch
silently mis-scores a model.

A string (`"llama_cpp"`) or an open `&str` discriminator compiles, but a typo or a newly
added case fails *silently* — a default branch swallows it.

## Decision

Model every closed set as a Rust `enum`. Dispatch via `match`, never a string compare
or `dyn`. Backends are selected by matching `BackendKind`, not by trait objects.

## Consequences

- Adding a variant makes the compiler flag **every** `match` that must change — the seams
  scream instead of defaulting. This is the property we want most on the scoring path.
- Enum-dispatch also removes the need for `async-trait`/`dyn` on the hot `InferenceBackend`
  path (zero-cost static dispatch).
- Wire formats (de)serialize via `serde` with explicit `rename_all`, keeping the JSON
  contract stable while the Rust side stays exhaustive.

## Alternatives considered

- **String/`&str` tags:** flexible but defeats exhaustiveness — rejected for any value the
  scorer branches on.
- **`Box<dyn Trait>` backends:** runtime polymorphism we don't need; the set is closed.

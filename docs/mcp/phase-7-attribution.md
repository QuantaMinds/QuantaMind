# Phase 7 — Error + attribution oracle (Track A)

**Goal:** whose fault was a tool-call failure — **model**, **config**, or
**server**? Uses MCP's two error channels as the cheap, answer-key-free signal.
With P6, **Track A now scores a user's own `mcp.json`**: schema-valid rate +
three-way attribution, no controlled world needed.

**Code:** `backend/src/inference/mcp/oracle_error.rs`.

- `wire_outcome(response)` → `Ok{is_error}` / `ProtocolError{code}` / `Transport`.
- `attribute(schema_check, wire)`:
  - client-side schema failure (hallucinated / bad args) → **Model**;
  - `result` `isError:true` → **Server** (tool ran and failed — coarse: a
    model-supplied bad value also lands here);
  - `-32601` (unknown *method*) → **Config**; `-32602` (unknown *tool* / invalid
    params, *after* our schema check passed) → **Config**;
  - `-32603`/server-defined, or `Transport` → **Server**.

**Verified corrections baked in:** unknown *tool* is `-32602`, not `-32601`
(that's unknown *method*); `-32602` is model-vs-config ambiguous, resolved to
config only because our client-side schema check already passed. Attribution is
coarse by design — finer is a ~50%-accurate research problem.

## Results — DONE
3 tests (wire-outcome classification of all three shapes; bad call → Model
regardless of wire; valid call attributed by wire incl. the -32601/-32602/-32603
split). Full lib suite 1178 green. **→ Track A complete.**

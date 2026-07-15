# Phase 6 — Schema oracle (Track A)

**Goal:** answer-key-free grading of whether a model's call *conforms* to the
tool's `inputSchema` — the cheap, high-value check that works on ANY server
without knowing the task. Kept strictly separate from "correct" (a schema-valid
call can still be the wrong call).

**Code:** `backend/src/inference/mcp/oracle_schema.rs`.

**Design note (deviation from the plan's "extend `validate_call`"):** the eval
`endstate::validate_call` is flat depth-1, 6 primitive types, and doesn't reject
unknown args — too weak for real MCP `inputSchema`. Built a proper recursive
JSON-Schema-subset validator instead (`validate_call` stays for the eval path).

- `check_call(tools, call)` → `Valid` / `UnknownTool` (hallucinated → a model
  fault caught client-side before it becomes a server `-32602`) / `Invalid(reasons)`.
  Resolves namespaced names.
- `validate_against_schema`: recursive over `type`, `properties`, `required`,
  `enum`, `additionalProperties:false` (rejects hallucinated args), array `items`;
  unknown keywords lenient.
- `SchemaScore` / `score_calls` → the **schema-valid rate**.

## Results — DONE
7 tests (valid, namespaced, hallucinated→UnknownTool, missing-required +
wrong-type, additionalProperties:false rejects an extra arg, nested
object/enum/array-items, aggregate rate). Full lib suite 1178 green.

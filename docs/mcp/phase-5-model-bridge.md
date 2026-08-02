# Phase 5 — Bridge MCP tools → local model (single-turn)

**Goal:** a real local model sees the MCP tools, emits a call, and the bridge
executes it against a real MCP server — on **both** llama.cpp.

**Code:** `backend/src/inference/mcp/bridge.rs` (in `inference/`, not `mcp/`, so
the pure protocol client stays free of inference — inference depends on the
client, the natural direction).

## Step 1 — Schema mapping
`mcp_tools_to_native(tools)` builds the native `tools` array
(`{type:function, function:{name, description, parameters}}`); MCP `inputSchema`
maps 1:1 to `function.parameters`. Same shape feeds llama.cpp `/v1/chat/completions` and the
OpenAI `/v1` path.

## Step 2 — Call selection (native + text fallback)
`select_calls(native_calls, content)`: use the native `tool_calls`; if empty,
scan the assistant `content` for a text-embedded call via the eval
`toolcall::parse::extract_calls` (the common local-model failure where the call
lands in prose). `stream:false` (llama.cpp drops tool calls under streaming).

## Step 3 — Dispatch + execute + warn
`single_turn(backend, …)` matches `BackendKind`: llama.cpp `llama_cpp_chat::
chat_with_tools` / llama.cpp `llama_chat::chat_with_tools` (both return the shared
`ChatResult`); vLLM rejected (no native tool API). Each call → `execute_call` →
`client.call_tool` → `flatten_content` (inert text) + `is_error` recheck.
`assess_tool_capability`/`capability_warning`: sub-3B or unknown models get a
warning — silent tool-drop and quantization schema-degradation are documented
failure modes.

## Results — DONE (both backends, live)

- 4 bridge unit tests (native shape, capability reliable/weak/unknown incl. the
  11b-≠-1b guard, native-then-text `select_calls`, `flatten_content`). Full lib
  suite green.
- **Live, real MCP server + stubbed model** (`bridge_executes_a_stubbed_call…`):
  execute+inject proven, both channels (`isError` false / true).
- **Live, REAL model on BOTH backends** (`bridge_single_turn…`):
  - **llama.cpp `qwen3.5:9b`** → emitted `read_text_file` → bridge read the seeded
    file (`bluebird`). warning=None.
  - **llama.cpp `qwen2.5-coder-7b`** (llama-server `/v1` + `--jinja`) → emitted
    `read_text_file` → read `bluebird`. warning=None.
  - No orphaned servers after either run.

**Real-world friction captured:** a locally-imported llama.cpp GGUF
(`qwen2.5-coder-7b-instruct:q4_k_m`) returned HTTP 400 `does not support tools` —
it lacked a tool-calling **template** (an import artifact, not a size limit). The
bridge surfaced it as a loud error. Capability (template) ≠ reliability (size);
official tags of any size advertise tools, size only affects call correctness.

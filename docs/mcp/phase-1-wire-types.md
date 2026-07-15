# Phase 1 — Wire types from captured bytes

**Goal:** pure serde types (no I/O) for JSON-RPC 2.0 + the MCP messages this
client sends/receives, modeled from `fixtures/mcp/*` — not the spec. The test
oracle is: every captured fixture must round-trip.

**Code:** `backend/src/mcp/wire.rs` (+ `wire_tests.rs`), `backend/src/mcp/mod.rs`,
`pub mod mcp;` in `backend/src/lib.rs`.

**Design rules pinned from the fixtures:**
- **Two error channels, distinct types.** A protocol failure is a top-level
  `error{code,message,data?}` (`error_method_not_found.json`, `-32601`). A tool
  failure is an in-band `result` with `isError:true`
  (`error_tool_inband.json`). Response is therefore `result` **xor** `error`;
  `CallToolResult.is_error` is a separate, always-rechecked flag.
- **Unknown-field tolerance is mandatory.** No `deny_unknown_fields` anywhere.
  The real `tools_list.json` already carries fields we don't model (`execution`,
  `$schema` inside `inputSchema`); a strict parser would drop every tool — a
  documented real-world failure. Unknown `content` block `type`s fall to a
  tolerant `Other`.
- **Capabilities are all optional.** `initialize.json` advertises **only**
  `tools`; the client keys behavior off what's present, never assumes.
- **`id` is number-or-string** per JSON-RPC; we allocate integers, but the type
  accepts both so an echoed/foreign id parses.

## Step 1 — JSON-RPC envelopes
`RequestId` (untagged num|string, `Eq+Hash` for the Phase 2 correlation map),
`Request`, `Notification` (no `id`), `Response` (`jsonrpc` + `id` + flattened
untagged `ResponsePayload::{Success{result}, Failure{error}}`), `JsonRpcError`,
standard error-code consts, `method::*` name consts.
Test: `error_method_not_found.json` → `Failure`, code `-32601`; a result payload
→ `Success`; both round-trip; `RequestId` num & string round-trip.

## Step 2 — Initialize types
`InitializeParams`, `InitializeResult`, `ClientCapabilities`,
`ServerCapabilities` (every field `Option`), `Implementation`.
Test: `initialize.json` request→`InitializeParams` (protocol `2025-06-18`);
response→`InitializeResult` advertising `tools` only, `serverInfo.name ==
"secure-filesystem-server"`.

## Step 3 — Tool + call-result types
`Tool` (`inputSchema` required, `title`/`description`/`outputSchema`/`annotations`
optional), `ToolAnnotations` (incl. `readOnlyHint`), `ToolsListResult`,
`CallToolParams`, `CallToolResult` (`is_error()` accessor), `ContentBlock`
(tagged by `type`, `Other` fallback).
Test: all 14 tools parse; `read_file` present + deprecated title;
`read_text_file` has `readOnlyHint:true`; `tools_call.json` → text content +
`structuredContent`; `error_tool_inband.json` → `is_error()==true`; injected
unknown field/content-type still parse.

## Verify / live
"Live" for a pure-types phase = the captured **real bytes** round-trip. Run the
whole lib suite (`~/.cargo/bin/cargo test --lib`), then record the actual pass
output and any shape surprises below.

## Results — DONE

`~/.cargo/bin/cargo test --lib mcp::` → **12 passed, 0 failed**. Full lib suite
→ **1148 passed, 0 failed** (new `mcp/` folder is 3 files, well under the ≤10
folder-taxonomy limit; no regression).

Real-byte facts the fixtures confirmed (the point of modeling from bytes):
- **`flatten` + untagged `result`-xor-`error` round-trips both directions**
  (`response_roundtrips_both_arms`) — the one design risk, retired.
- All **14** real filesystem tools parse; `read_file` carries the deprecated
  title; `read_text_file` carries `readOnlyHint:true` / `openWorldHint:false`.
- The two error channels are genuinely distinct types: `error_method_not_found`
  → `Failure{-32601}`; `error_tool_inband` → `Success` protocol channel but
  `CallToolResult::is_error() == true`.
- Unknown tool fields (real server's `execution`, `$schema`) and unknown
  `content` block types parse instead of dropping the tool — the "client drops
  all tools" bug is structurally prevented.

`initialize` advertises **only** `tools` — so the Phase 2 client must gate
`tools/list` on `ServerCapabilities::has_tools()`, not assume.

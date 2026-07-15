# MCP fixtures — captured from a real server

These are **real captured bytes**, not spec-derived hand-writes. The Rust
JSON-RPC/MCP types must be modeled from *these shapes*, because MCP clients and
servers routinely violate their own spec — model from the wire, not the doc.

## Provenance

- **Server:** `@modelcontextprotocol/server-filesystem` (reports
  `serverInfo.name = "secure-filesystem-server"`, `version 0.2.0`), spawned via
  `npx -y` over **stdio** (newline-delimited JSON-RPC 2.0).
- **Client:** a throwaway Node harness performing the real handshake.
- **Negotiated protocolVersion:** `2025-06-18` (client offered it; server echoed it).
- **Capture date:** 2026-07-13.
- **Neutralized:** absolute sandbox paths that embedded the local username were
  replaced with `/tmp/mcp-sandbox/...` in `tools_call.json` and
  `error_tool_inband.json`. Nothing else was altered. Every field name, nesting,
  and value shape is byte-faithful to the server output.

Each file wraps the exchange as `{ transport, request, response }`.

## What each fixture pins

| File | Pins |
|------|------|
| `initialize.json` | Handshake. Note the response advertises only `capabilities.tools` — **no** `prompts`/`resources`/`logging`. A client must key off advertised capabilities, not assume. `serverInfo` carries a *different* name than the npm package. |
| `tools_list.json` | 14 real tool schemas. Note: draft-07 `inputSchema`, optional `title`, `annotations` (`readOnlyHint`/`openWorldHint`), an `execution.taskSupport` field, and an `outputSchema` — several fields the spec's minimal examples omit. `read_file` is present but marked **Deprecated** in favor of `read_text_file`. |
| `tools_call.json` | Success shape: `result.content[]` (typed blocks, here `{type:"text"}`) **plus** a newer `result.structuredContent`. A robust client reads `content[]` and treats `structuredContent` as optional. |
| `error_tool_inband.json` | **Tool-level failure is in-band:** `result.isError = true` with the reason inside `content[]`. It is *not* a JSON-RPC error. Client must inspect `isError` on every successful-looking `tools/call`. |
| `error_method_not_found.json` | **Protocol-level failure:** top-level `error { code: -32601, message }`, no `result`. This is the JSON-RPC error channel — distinct from the in-band tool error above. The Rust response type must be an untagged either/or of `result` xor `error`. |

## Regenerating

Harnesses live in the session scratchpad (`capture-mcp.mjs`,
`capture-mcp-errors.mjs`). They spawn the server, do
`initialize → notifications/initialized → tools/list → tools/call`, and dump
each request+response. Re-run against any stdio MCP server to refresh.

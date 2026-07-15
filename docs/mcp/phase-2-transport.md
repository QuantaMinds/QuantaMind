# Phase 2 — stdio transport + minimal client

**Goal:** spawn a real MCP server over stdio, complete
`initialize → notifications/initialized → tools/list → tools/call`, and get real
bytes back — with the framing/corruption failure modes handled loudly.

**Code:** `backend/src/mcp/transport.rs` (spawn + framing + id-correlation),
`backend/src/mcp/client.rs` (handshake + list/call), live test
`backend/tests/mcp_live.rs` (`#[ignore]`, spawns the real filesystem server).

**Design, pinned from the research + fixtures:**
- **stdout is protocol; stderr is logs.** stdin+stdout are `Stdio::piped()`
  (bidirectional JSON-RPC); stderr is drained on a thread into a **redacted**
  bounded tail. A **non-JSON line on stdout** (the #1 real-world bug — servers
  logging to stdout) is recorded as the "first garbage" and surfaced in the
  request error, never hung on.
- **Framing:** compact one-line JSON, `\n`-terminated, no batch arrays. A
  blocking stdout reader thread parses each line and routes by shape:
  `{id, result|error}` → a `Response` correlated by id to a
  `tokio::sync::oneshot`; `{method, …}` (notification / server request) → ignored
  for now (sampling/roots unimplemented).
- **Capability gating:** `list_tools()` refuses unless the negotiated
  `initialize` result advertised `tools` (`has_tools()`).
- **Two error channels:** `call_tool` returns `CallToolResult` and the caller
  rechecks `is_error`; a protocol failure surfaces as the JSON-RPC `error` arm.

**Process-group caveat (deferred to P3):** `Host::apply_spawn_flags` is a **no-op
on Unix**, so a child spawned via `npx` that reparents to `node` is NOT in our
process group — a plain kill can orphan `node`. P2's `kill()` is best-effort
(graceful_stop + kill + wait); **P3** adds Unix `setpgid`/group-kill, readiness
gating, and reaping registration.

## Step 1 — Transport (spawn + framing + correlation)
`McpTransport::spawn(program, args)` via `Host::command`, piped stdio, redacted
stderr tail; stdout reader thread + `route_line`; `request`/`request_timeout`
(alloc id → insert oneshot → write line → await with timeout, mapping
sender-dropped→"server closed" and timeout→garbage-aware diagnostic); `notify`
(no id); `kill`. Unit-test `route_line`: correlates a response by id, ignores a
notification, records non-JSON as garbage.

## Step 2 — Handshake
`McpClient::initialize()`: send `initialize` (offer `LATEST_PROTOCOL_VERSION`);
accept a same-version or counter-offered result, error on `-32602`; store
negotiated version + `ServerCapabilities`; send `notifications/initialized`.

## Step 3 — list/call
`list_tools()` gated on `has_tools()` → `ToolsListResult`; `call_tool(name,args)`
→ `CallToolResult` (caller rechecks `is_error`).

## Verify / live
Unit: `route_line` correlation/garbage. **Live** (`cargo test --test mcp_live --
--ignored`): spawn `npx -y @modelcontextprotocol/server-filesystem <tmp>`;
`list_directory` → real entries; `read_text_file` → real content; `/etc/hosts` →
`is_error:true`; unknown method → `-32601`. Inspect real output.

## Results — DONE

- Unit: `cargo test --lib mcp::` → **18 passed** (6 new transport + 12 wire).
  Full lib suite **1154 passed, 0 failed**.
- **Live** (`cargo test --test mcp_live -- --ignored --nocapture`): real
  round-trip against `secure-filesystem-server 0.2.0`, protocol negotiated to
  `2025-06-18`, all **14** tools listed, `read_text_file` returned real content,
  and **both error channels** fired — `isError:true` for a sandbox-escape read,
  `-32601` for an unknown method. `pgrep -f server-filesystem` after teardown →
  **clean, no orphan**.

Note on teardown: it was clean here (killing the `npx` child closed the server's
stdin → EOF exit), but on Unix `Host::apply_spawn_flags` is a no-op, so this
isn't guaranteed for every server — **P3** adds real group-kill + reaping.

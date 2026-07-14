# Phase 12 — Test-page MCP path (UI)

**Goal:** the front door to everything the backend now does — connect servers,
choose the track, build/run a task, watch it live, read the verdict.

**Code:** `frontend/src/features/mcp/` (`state/mcpStore.ts`,
`components/McpConnectPanel.tsx`); IPC in `shared/ipc/mcp/servers.ts` (P4).

## Delivered — slice 1 (connect + two-track foundation)
- **`mcpStore`** (Zustand): server list CRUD + `probe` (the loud "N tools
  discovered" preflight — a bad command / stdout-polluting server surfaces here,
  not mid-run) + the `mode` (`controlled` | `byo`) two-track selector.
- **`McpConnectPanel`**: the two-track selector rendered so **Bring-Your-Own is
  visibly distinct** — it advertises *format + attribution only*, with
  "No task-completion verdict — we have no answer key" (seeing ≠ scoring, the
  honest rule made visual); a server list with Connect→"✓ N tools discovered" /
  "✗ loud error"; an add-server form.
- **5 vitest tests** (refresh, probe-ok records N tools, probe-error is loud,
  addServer upsert+refresh, mode switch). `tsc` clean.

## Mounted into the Test page ✅
`EvalManager.tsx` COLLECTIONS selector now shows **`◉ MCP  ◯ Built-in  ◯ Custom
JSON`** (MCP first). `dataSource` gains an `mcp` value via an `mcpMode` override
(MCP has no collection selection); choosing MCP renders `McpConnectPanel` in the
list area. tsc clean; EvalManager + mcp suites 45 tests green.

## Layout + builder ✅
- **Center = connect + build** (`McpCenterPanel`): "Connect your MCP tools" +
  the guided builder; **Sidebar = the MCP task list** only. Shared via
  `mcpStore.active`.
- **Guided builder** (`McpTaskBuilder`, Screen 4): three sections that *are* the
  task JSON — Task (instruction) · Set up the world (fs seed files / db setup SQL)
  · Check the result (present/absent/content, or DB query assertions) · pass^k.
  Save writes one `McpTaskDef` (pure `toTaskDef`, unit-tested) into the store; it
  appears in the sidebar. BYO mode greys the builder out (attribution-only note).

## Run pipeline ✅ (both tracks, real model from the global header)
- Backend: `inference/mcp/agent::BackendDriver` (real multi-turn driver —
  `bridge::chat` + transcript), `inference/mcp/bridge::chat` (dispatch), and
  `commands/mcp/run_cmd.rs`:
  - `run_mcp_world_task(model, backend, task, max_steps)` → seeds a fresh world
    per run, drives the model, grades the end-state → `{k, passes, ready,
    pass_rate, failures}` (pass^k). Endpoint resolved from `backend`.
  - `run_mcp_byo(model, backend, server_id, instruction)` → one model turn vs the
    user's server → schema-valid rate + model|config|server attribution + per-call
    trace.
- Frontend: `shared/ipc/mcp/run.ts`; the center shows **Model: X · backend (from
  the global header)** — no separate picker. `McpWorldRunner` (task list → Run →
  READY/CONDITIONAL/NOT READY verdict + failures) and `McpByoRunner` (instruction
  + server → schema-valid rate + attribution). Quick-add chips for the reference
  filesystem/sqlite servers.
- **Live-verified**: the real `BackendDriver` scored a world task end-to-end —
  Ollama `qwen3.5:9b` created `result.txt` across fresh worlds → **2/2 ready**.

## Remaining (honest status)
- **Streaming live trace** (Screen 5): the verdict is returned at the end; a
  per-turn stream (call + schema check + oracle, changing per-run path) would need
  a progress-event channel like the eval batch emitter.
- **Upload JSON / template** doors (same `McpTaskDef` format the builder emits).

The backend for all of the above is complete and live-verified (P1–P11); this
slice is the tested UI foundation it plugs into.

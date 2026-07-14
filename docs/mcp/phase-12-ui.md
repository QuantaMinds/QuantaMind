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

## Remaining slices (honest status)
- **Run pipeline**: a Tauri command (`run_mcp_task`) + a real multi-turn
  `BackendDriver` (chat_with_tools + transcript) so "Run" scores a built task —
  the backend `score_fs_task`/`score_db_task` exist but aren't yet exposed to IPC.
- **Live trace** (Screen 5) + **Verdict** (Screen 6): stream per-run steps
  (call + schema check + oracle) and show pass^k + attribution + replay/export.
- **Upload JSON / template** doors (same `McpTaskDef` format the builder emits).

The backend for all of the above is complete and live-verified (P1–P11); this
slice is the tested UI foundation it plugs into.

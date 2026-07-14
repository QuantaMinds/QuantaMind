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

## Remaining slices (honest status)
- **Guided builder** (Screen 4): the Task / Set-up-the-world / Check-the-result
  form that *writes the one task JSON* (`world` + `oracle`), with the world-builder
  greyed out for BYO servers.
- **Upload JSON / template** doors (the same format the builder emits).
- **Live trace** (Screen 5): per-run changing temp path, each call + schema check
  + oracle verdict.
- **Verdict** (Screen 6): pass^k + three-way attribution + replay + export.
- **Mount** into `EvalPage`'s source picker as the third tab. Not done here on
  purpose — a blind edit to the 334-line page risks the build; it wants a
  focused follow-up with the page in view.

The backend for all of the above is complete and live-verified (P1–P11); this
slice is the tested UI foundation it plugs into.

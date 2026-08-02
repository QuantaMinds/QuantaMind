# Phase 4 — Registry, secrets, security gate

**Goal:** persist a list of MCP servers, hold live connections as managed state
that is reaped on exit, and route every security-sensitive input through the
app's existing ports (`fs_guard`, `SecureSecrets`). Exposes the commands the UI
(P12) and Track A/B use.

**Folder note:** the on-disk registry lives in a **`persistence/mcp/`
subfolder** (mirrors `persistence/jobs/`) — `persistence/` is
already at the folder-taxonomy limit, so a subfolder avoids worsening it.

## Step 1 — Registry persistence  → `persistence/mcp/servers.rs`
`McpServerConfig { id, command, args, env_keys, roots, enabled }` +
`McpRegistry { servers }` → `mcp_servers.yaml` (load/save mirror
`user_settings`). **No secret VALUES on the struct** — only `env_keys` (the
*names* of env vars whose values live in the keychain), so no strip step needed.
`validate` (non-empty/unique id, non-empty command). `canonical_roots()`
canonicalizes each root (resolve symlinks; must be an existing dir) — the
EscapeRoute-safe boundary, never a raw prefix-match.

## Step 2 — Secrets seam  → `secrets.rs`
A helper to store/fetch a server's env-var values in the OS keychain keyed by
`(server_id, env_key)`; only the key *names* are persisted to yaml.

## Step 3 — In-memory registry + managed state  → `mcp/registry.rs`
`McpServerState` (Tauri managed) holding live `McpClient`s keyed by server id;
Drop-kills; **per-server tool namespacing** (anti cross-server shadowing);
**hash-pin** tool definitions on first connect, diff on reconnect /
`tools/list_changed` (rug-pull detection).

## Step 4 — App-lifecycle reaping (moved from P3)  → `commands/app_lifecycle.rs`
Wire `McpServerState` into `reap_managed` / `reap_on_exit` / signal-reaper. Note
`sweep_orphans` matches `.quantamind` in the cmdline — MCP servers (npx/node)
won't, so **group-kill on our tracked PIDs (P3) is the real defense**, documented.

## Step 5 — Commands + IPC  → `commands/mcp/`, `frontend/src/shared/ipc/mcp/`
`add/list/remove/set_enabled/probe` (probe = connect + `tools/list` → "N tools
discovered"; the doctor/preflight moment). Register in `lib.rs generate_handler!`
+ `.manage(McpServerState)`. Untrusted descriptions/instructions/results stay
inert. Install command shown verbatim; flag `sudo`/`rm -rf`/SSH-key patterns.

## Verify / live
Registry round-trip + validation (empty/dup id, empty command) + `canonical_roots`
(valid dir ok; nonexistent/file → err; symlink resolved). Live: `probe` a real
filesystem server via the command path → "14 tools discovered"; a root outside an
allowed base rejected; a mutated tool description trips the hash-pin.

## Results

- **Step 1 — DONE.** `persistence/mcp/servers.rs`: `McpServerConfig` /
  `McpRegistry` + yaml load/save + `validate` + `canonical_roots`. 6 tests
  (round-trip, missing→empty, `enabled` defaults true, names-not-values,
  validation rejects empty/dup id + empty command, `canonical_roots` resolves
  dirs / rejects nonexistent+file). Full lib suite **1161 green**. Placed in a
  `persistence/mcp/` subfolder so the pre-existing `persistence/` folder-taxonomy
  debt (already 15, unrelated to this work) is not worsened.
- **Step 2 — DONE.** `secrets::mcp_env_key(id, var)` — keychain key for a server
  env-var value; only the name is persisted. Transport gained `spawn_with_env` /
  client `connect_with_env` to pass those values to the child.
- **Step 3 — DONE.** `mcp/registry.rs`: `namespaced`/`split_namespaced` (anti
  shadowing), `tool_fingerprint` + `PinnedTools::diff` → `ToolDiff::is_rug_pull`
  (changed/removed = rug-pull, added ≠ alarm), `McpServerState` managed set +
  Drop-kill. 3 tests.
- **Step 4 — DONE.** `reap_managed` now kills `McpServerState` (+ noted the
  `sweep_orphans` marker gap → P3 group-kill is the real defense).
- **Step 5 — DONE.** `commands/mcp/mcp_cmd.rs`: `list/upsert/remove/
  set_enabled/set_secret/probe`; registered in `lib.rs` + `.manage`. Frontend
  `shared/ipc/mcp/servers.ts` wrappers (zod). `probe` = connect + list → "N tools
  discovered".

**Verified:** 28 MCP unit tests; full lib suite **1164 green**; frontend tsc
clean; **live** — the probe flow (registry config → `canonical_roots` →
`connect_with_env` → `tools/list`) lists all 14 real tools, no orphan.

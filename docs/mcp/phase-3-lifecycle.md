# Phase 3 — Lifecycle hardening

**Goal:** a spawned MCP server can never orphan a child, and a slow-booting
server is *waited for* (readiness = the `initialize` response), not dropped.

**Code:** `backend/src/mcp/transport.rs` (process-group isolation + group-kill +
`pid()`), `backend/src/mcp/client.rs` (`connect_with_timeout`).

**Why the P2 teardown wasn't enough:** `Host::apply_spawn_flags` is a **no-op on
Unix**, so a server launched via `npx` (which reparents to `node`) or one that
forks a worker is NOT in our process group — a plain kill of the direct child
leaves the grandchild orphaned (the real "1,300 zombie MCP processes" failure).

## Step 1 — Process-group isolation + group-kill (Unix)
At spawn, make the child a **group leader** with the *safe* stdlib
`std::os::unix::process::CommandExt::process_group(0)` (no `unsafe`, so it clears
`#![deny(unsafe_code)]`) → its pgid equals its pid. `kill` then signals the whole
group (`kill -TERM -- -<pgid>`, grace, then `kill -KILL -- -<pgid>`) and reaps the
`Child`. Windows keeps the existing path (`apply_spawn_flags` sets
`CREATE_NEW_PROCESS_GROUP`; `Host::graceful_stop` sends `CTRL_BREAK` to the
group). Add `pid()` for diagnostics/tests.

> **The `--` is load-bearing** (`group_signal_argv`). Without it, procps `kill`
> (every Linux distro) reads the leading-dash pgid as *bundled short options*
> rather than a negative pid, and signals **the caller's own process group**.
> Measured on ubuntu-22.04, child correctly leading group 4192, caller in 4191:
> `kill -TERM -4192` killed the **caller** (SIGTERM, exit 143) and left the group
> **alive** — the exact inverse of this step's goal: it orphaned the server and
> killed the app. `kill -TERM -- -4192` exits 0, kills the group, caller lives.
> BSD `kill` (macOS) parses the bare form correctly, which is why this hid until
> CI's Linux job started dying mid-suite. `--` is accepted by both, so one argv
> is right everywhere. `pgid == 0` is refused outright: `kill -- -0` is
> `kill(0, sig)` = "signal my own group", which can only ever be a bug.

## Step 2 — Readiness = the initialize response
MCP has no `/health`; readiness *is* a successful `initialize`. `connect` already
blocks on it; add `connect_with_timeout` so a slow server gets a generous,
caller-set window and a timeout fails **loud** (the transport's garbage-aware
diagnostic), never silently drops the server's tools.

## Step 3 — Windows `npx` shim resolution
`npx` is a `.cmd` shim; bare `CreateProcess("npx")` → `ENOENT`. On Windows, wrap a
bare (extension-less, non-absolute) program via `cmd /c …` so `npx`/`npm`/`node`
launch. **Untested on Windows in this session** — flagged for a Windows live run.

## Deferred to P4 (sequencing correction)
`McpServerState` managed-registry + `reap_managed`/`reap_on_exit`/signal-reaper
wiring + `sweep_orphans` signature move to **P4**, where the registry + Tauri
commands that actually *hold* long-lived servers live — there is nothing to reap
until then. The per-transport `Drop`-kill (done in P2) already prevents a leaked
*transient* server (the Track-B per-run case).

## Verify / live
- **Group-kill (deterministic, in the normal suite, Unix):** spawn a transport on
  `sh -c 'sleep 300 & wait'` (forces a distinct grandchild in the group), assert
  the group is populated, `kill()`, assert the group is **empty** — proves the
  grandchild died, not just the direct child.
- Re-run the P2 npx live test → still no orphan.

## Results — DONE

- Unit: `cargo test --lib mcp::` → **19 passed** (adds the group-kill proof).
  Full lib suite **1155 passed, 0 failed**.
- **Group-kill proof** (`kill_terminates_the_whole_process_group_leaving_no_orphan`):
  spawned `sh -c 'sleep 300 & wait'`, confirmed the group populated, `kill()`,
  confirmed `pgrep -g <pgid>` empty — the forked `sleep` grandchild died with the
  group, not orphaned. This is the fix a plain direct-child kill would have missed.
- **Live** npx round-trip re-run with the new group-kill → still passes,
  `pgrep -f server-filesystem` after teardown **clean**.

Windows `cmd /c` shim + `CREATE_NEW_PROCESS_GROUP` path is implemented but
**unverified on Windows this session** — needs a Windows live run before P12.

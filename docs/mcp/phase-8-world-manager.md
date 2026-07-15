# Phase 8 — World-manager (Track B)

**Goal:** a controlled, disposable world — a fresh sandbox we seed, with a REAL
MCP server scoped to *only* that sandbox — so we can drive real tools and grade
the sandbox's **end-state** (not the model's words). Because we author the seed,
the answer key is knowable even though the tool is real (τ-bench discipline).

**Code:** `backend/src/inference/eval/mcp/world.rs`.

**Design decisions:**
- The eval runner's `respond` is **sync** but MCP is **async**, so P8 builds the
  async world-manager + its `execute` seam directly; wiring it into the runner's
  pass^k loop (the sync-bridge decision) is **P10**.
- Track A oracles live in `inference/mcp/` (bridge-adjacent, no eval dep); Track B
  (this world + P10 fs oracle) lives in `inference/eval/mcp/`.
- **No `tempfile` dev-dep in production** — a tiny self-cleaning `ScratchDir`
  (pid + process counter, `remove_dir_all` on drop).
- **Safety here is isolation, not approval:** the server is confined to the
  disposable sandbox (seed paths pass through `fs_guard::ensure_within`), so a
  controlled-world run needs no human gate. The approval gate (P9) is for the
  user's REAL tools.

`McpWorld::filesystem(seed)`: fresh `ScratchDir` → `write_seed` → spawn
`server-filesystem` scoped to the **canonical** root → `execute(call)` per step →
`Drop` kills the server then removes the dir. `root()` returns the **canonical**
root (drivers must build paths from it).

## Results — DONE
- 3 unit tests (seed writes files + nested dirs; rejects `..`/absolute; scratch
  dirs unique + removed on drop). Full lib suite **1181 green**.
- **Live** (`--ignored`): seed `old.log`+`keep.txt` → real `write_file` →
  **the file actually exists on disk** (world mutated) → an outside-sandbox write
  is **refused** (isolation) → teardown removes the per-run dir. Two worlds get
  **distinct** dirs (fresh-per-run). No orphans, no stray dirs.
- **Live testing caught a real bug:** `root()` first returned the raw scratch
  path (`/var/folders/…T/`), but the server confines to the canonicalized
  `/private/var/…`, so writes were denied. Fixed to store/return the canonical
  root — a defect that would have silently broken real end-state scoring.

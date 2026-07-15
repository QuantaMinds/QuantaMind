# Phase 11 — DB world (sqlite)

**Goal:** a second controlled-world type — a seeded sqlite DB behind a real
sqlite MCP server — proving the world/oracle abstraction generalizes beyond the
filesystem.

**Code:** `inference/eval/mcp/oracle_db.rs` (DB oracle), `world.rs`
(`McpWorld::sqlite` + `db_path`), `score.rs` (`DbTask` + `score_db_task`).

- **Server:** `mcp-server-sqlite-npx` (reports `sqlite-manager 0.8.0`) — tools
  `read_query`, `write_query`, `create_table`, `list_tables`, `describe_table`.
- **Seed:** `DbSeed` SQL run once via the `sqlite3` CLI to build the initial DB.
- **Oracle:** `DbOracle` grades by running SELECTs **directly via the `sqlite3`
  CLI** — independent of the agent's tools (τ-bench reads the DB itself), so the
  model's claims can't fool it. (Windows needs `sqlite3.exe`.) No Rust sqlite dep.
- `McpWorld::sqlite(seed)` seeds a fresh temp DB + spawns the server scoped to it;
  `score_db_task` runs pass^k exactly like the fs path.

## Results — DONE
- 1 unit test (oracle grades a seeded DB via the CLI; skips if no `sqlite3`).
  Full lib suite **1195 green**.
- **Live**: seed `users(Bob)`, task "insert Alice", k=3 —
  - **Honest** model (`write_query INSERT`) → **3/3, ready**.
  - **Fake-done** (says "Added Alice", calls nothing) → **0/3, NOT ready**
    (`SELECT COUNT(*) WHERE name='Alice'` = 0, expected 1).
  - No orphans. Same grade-the-world discipline as the fs world, different world.

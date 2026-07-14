# Phase 10 — FS-state oracle + task-format + pass^k scoring (Track B)

**Goal:** turn the world-manager (P8) + loop (P9) into a real **pass^k
task-completion verdict** that grades the world's end-state — and catches
fake-done.

**Code:** `inference/eval/mcp/oracle_fs.rs` (the answer key), `score.rs` (task
format + pass^k runner).

## Task format (the plan's two new fields)
`McpTask { instruction, seed: FsSeed, oracle: FsOracle }` — `mock_result` is
gone; we grade the real world. `FsOracle { assert_present, assert_absent,
assert_content }`, `grade(root) -> FsVerdict`.

## pass^k scoring
`score_fs_task(task, make_driver, k, max_steps)`: k runs, a **fresh world each**
(pass^k independence), driver built per run, controlled world → auto-approve,
then grade the end-state. `McpScore::is_ready` = strict all-k-pass (a single
lucky pass is not readiness). Reuses the P8 world + P9 loop + this oracle rather
than retrofitting the sync eval runner (the async-native path with the same
pass^k semantics).

**Note:** `server-filesystem` has no *delete* tool, so the canonical demo is a
create/write task; the fake-done principle is identical.

## Results — DONE
- 5 unit tests (oracle present/absent/content pass+fail; strict-pass^k treats a
  lucky pass as not-ready). Full lib suite **1194 green**.
- **Live, the money test** (`passk_scoring_catches_fake_done`): task "create
  result.txt containing DONE", k=3:
  - **Honest** model (really writes) → **3/3, ready**.
  - **Fake-done** model (says "Done! I created result.txt" but calls nothing) →
    **0/3, NOT ready**, failure "result.txt missing" — caught by grading the
    world, not the claim.
  - No orphans; each run used a fresh disposable world.

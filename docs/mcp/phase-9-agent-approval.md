# Phase 9 — Multi-turn loop + approval gate (DANGEROUS)

**The first path where a real model drives a real side-effecting tool in a loop.**
**HARD PRECONDITION (met):** the iteration cap, the result-must-inject guarantee,
and the deny-by-default gate are built and pass their own tests BEFORE any live
run. These are backend-enforced, independent of any UI (P12's modal is only a
*source* of the external decision; if it's closed the backend still denies).

**Code:** `backend/src/inference/mcp/gate.rs` (policy), `agent.rs` (loop).

## The gate (deny-by-default)
`gate_decision(policy, call, external)`:
- **SandboxAutoApprove** — a controlled disposable world is safe by isolation →
  approve all (you can't gate k automated runs on human clicks).
- **RealToolsReadOnlyAuto** — auto-approve ONLY a read-only call on a *trusted*
  (hash-pinned) server; a write, or a read-only call on an untrusted server
  (poisoned `readOnlyHint`), needs an explicit external Approve.
- **DenyByDefault** — nothing runs without an explicit Approve.

`external = None` (no decision / timeout / closed UI / unknown) → **Deny**, always.

## The loop (cap + inject)
`run_loop(driver, executor, gate, max_steps)`: hard cap (a runaway that never
yields stops at the cap, honestly); a denied call is **not** executed but its
denial is **injected** so the model learns it was refused; every tool result
enters the next turn's transcript (result-must-inject — the anti-infinite-loop
guarantee). Generic over `TurnDriver`/`ToolExecutor` so the rails are tested with
fakes; `McpExecutor` is the real executor over an `McpClient`.

## Results — DONE
- **8 safety-rail unit tests**: gate — sandbox approves even writes; real-tools
  auto-approve only trusted read-only; a write is never auto-approved; absent
  decision → deny (every policy). Loop — cap stops a runaway; model-yield ends
  early; a denied call never reaches the executor + is injected; every result is
  injected. Full lib suite **1189 green**.
- **Live** (`--ignored`, deterministic — no model nondeterminism): a scripted
  model attempts one write against a real seeded world — **DENY → the world is
  NOT mutated**; **APPROVE → the file really exists** with the written contents.
  The gate governs real side effects. No orphans.

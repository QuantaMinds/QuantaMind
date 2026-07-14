# MCP validation — methodology & sources

Why QuantaMind grades **the world, not the model's words**, and the external
work that grounds the two-track design. Referenced by the oracle phases (P6–P11).

## The core move

When the tool is real you lose the answer key for its *output*. So you stop
grading output and grade **the state of a world you control** — you built it, so
you know the correct end-state. Grading splits into three independent gates plus
attribution:

- **Structural gate** (needs nothing task-specific): did the call conform to the
  tool's schema? Works on any server → Track A.
- **Outcome gate** (needs a controlled world, not an output key): did the world
  change into the goal state? → Track B.
- **Trajectory gate** (needs a golden trace): was the path sane/safe, not just
  the destination? → deferred (4th rung).
- **Attribution**: model vs config vs tool/server.

## τ-bench — the canonical state-based grader

*"τ-bench: A Benchmark for Tool-Agent-User Interaction,"* Yao, Shinn, Razavi,
Narasimhan (Sierra), [arXiv 2406.12045](https://arxiv.org/abs/2406.12045),
[repo](https://github.com/sierra-research/tau-bench). Grades by comparing the
**final database state to a goal state**, not the transcript (verified in
`tau_bench/envs/base.py`):

- **Answer key = golden actions, replayed.** `calculate_reward()` hashes the
  agent's end DB, then **reloads a fresh DB and replays the task's annotated
  golden `actions`** to produce `gt_data_hash`; `reward = 1.0` iff the SHA-256 of
  the canonicalized DBs match. It is *not* a stored target hash. Read-only
  "information" tasks additionally check required strings in a `RESPOND` action —
  the one place chat text is scored, and only for non-mutating tasks.
- **pass^k = reliability, and it FALLS with k.** Unbiased estimator over n trials
  with c successes: **pass^k = E_task[ C(c,k) / C(n,k) ]** — probability that a
  random k-subset of runs *all* pass. Opposite of pass@k (best-of-k, rises with
  k). SOTA agents are "quite inconsistent (pass^8 < 25% in retail)." *(Exact
  paper LaTeX not extractable; the combinatorial form is HumanEval-standard and
  multiply corroborated — treat as secondary-sourced.
  [philschmid](https://www.philschmid.de/agents-pass-at-k-pass-power-k),
  [hippocampus-garden](https://hippocampus-garden.com/pass_k/).)*
- **τ²-bench** ([arXiv 2506.07982](https://arxiv.org/abs/2506.07982)) adds a
  dual-control (both user and agent use tools) Dec-POMDP telecom domain;
  **τ³** ships 75+ fixes to the *original* airline/retail tasks (they had
  material annotation defects); Amazon's
  [tau2-bench-verified](https://github.com/amazon-agi/tau2-bench-verified) is a
  corrected fork.

**How QM maps onto this:** our fs/db oracles use explicit end-state assertions
(`assert_absent`/`assert_present`/row-exists) rather than golden-action replay —
simpler and clearer for a user authoring a task. The existing `RequireEndState`
subset-match grader + pass^k loop are the τ-bench mechanics we reuse.

## Grade the world, not the transcript — why "it said done" is unsafe

- Anthropic, *Demystifying evals for AI agents*
  ([link](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents)):
  *"A flight-booking agent might say 'Your flight has been booked'… but the
  outcome is whether a reservation exists in the environment's SQL database."*
  Grade *"an order was actually placed, not just that the confirmation page
  appeared."*
- **Self-report is not just noisy but corruptible.** METR caught frontier models
  faking passes by stubbing the grader / monkey-patching / tampering with
  `time.time` ([metr.org](https://metr.org/blog/2025-06-05-recent-reward-hacking/));
  the Anthropic–OpenAI joint eval documents self-serving hallucinations
  ([link](https://alignment.anthropic.com/2025/openai-findings/)). This is our
  "fake-done" failure mode with external corroboration — a transcript "done" is
  an *unverified, gameable* claim.

## Structural gate — the free check (Track A)

MCP hands you the schema: `tools/list` gives each tool an `inputSchema` and
optional `outputSchema` (JSON Schema **2020-12** when `$schema` absent); the spec
says *"Clients SHOULD validate structured results against
[`outputSchema`]"* ([MCP tools spec](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)).
So both directions are answer-key-free: validate the model's *call* against
`inputSchema`, and (optionally) the server's *structured result* against
`outputSchema`. Berkeley BFCL is the reference AST-match approach
([blog](https://gorilla.cs.berkeley.edu/blogs/8_berkeley_function_calling_leaderboard.html));
its only truly answer-key-free checks are **schema conformance** and
**relevance/hallucination** (did it invent a tool / call when it shouldn't).
**Keep "schema-valid" and "correct" as separate gates** — constrained decoding
guarantees shape, never the right tool/args
([Databricks](https://www.databricks.com/blog/unpacking-function-calling-eval)).

## Attribution — the three-way split (Track A)

MCP's two-layer error model is the cleanest boundary you get for free
([spec](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)):

| Wire shape | Attribution |
|---|---|
| top-level JSON-RPC `error` (e.g. `-32601` unknown *method*, `-32603` internal) | **protocol/config** — never validly reached a working tool |
| `result` with `isError:true` | **tool/server** — the tool ran and failed |
| `result`, no `isError`, but wrong tool/args | **model** — well-formed but wrong |

**Correction that changes P7:** unknown *tool* → **`-32602` (Invalid params)**,
NOT `-32601`, because `tools/call` is a valid method and the unknown tool is a
bad *parameter value*. And **`-32602` is itself ambiguous** (model invented bad
args vs config/schema mismatch) — disambiguate by inspecting schema+args.
([mcpevals.io](https://www.mcpevals.io/blog/mcp-error-codes),
[JSON-RPC 2.0](https://www.jsonrpc.org/specification).) Finer automated
attribution (*Who&When*, [arXiv 2505.00212](https://arxiv.org/abs/2505.00212)) is
a ~50%-accurate research frontier — don't oversell it beyond the wire boundary.

## Controlled-world setup — reset must be TOTAL

Pattern across SWE-bench (Docker + git reset), WebArena (container reset),
terminal-bench (asserts final container state), MCPWorld/MCPMark
(snapshot/restore): **known seed → act → assert state diff → hard reset before
the next trial.** Caveats that bite:

- **Reset must cover files + git history + caches + clock + RNG**, not just
  visible files — Anthropic saw Claude read prior-trial git history for an unfair
  advantage. Our **fresh temp dir / fresh temp DB per run** gives a byte-identical
  seed for FS/DB worlds; if a world ever uses time/randomness, inject a fixed
  clock/seed.
- Each of the k trials must start byte-identical or you measure drift, not
  reliability.

## The gap this fills

Among MCP-native eval tooling (mid-2026), only **MCPWorld**
([arXiv 2506.07672](https://arxiv.org/abs/2506.07672)) genuinely does
seeded-world + end-state scoring, and it's small/preliminary. **MCP-Universe**
([arXiv 2508.14704](https://arxiv.org/abs/2508.14704)) does execution-based
outcome checks but against *live* external services (non-deterministic).
Salesforce **MCPEval**, Accenture **MCP-Bench**, LastMile **mcp-eval**, etc. are
LLM-judge or tool-call-trajectory matching — **not** controlled-world end-state.
The mature seeded-DB design (**τ-bench**) is **not MCP**. Anthropic's **MCP
Inspector** is a debugger, not an eval tool. → A local, controlled-world MCP
harness that scores end-state with pass^k is largely unbuilt territory.

## Deferred: trajectory / golden-trace (4th rung)

Record the correct tool-call sequence once, diff future runs — catches
"right answer, dumb/dangerous path." LangSmith `agentevals` has the clearest mode
taxonomy (strict/unordered/subset/superset;
[repo](https://github.com/langchain-ai/agentevals)); Vertex adds
precision/recall for partial credit. **Pitfall:** strict exact-match has high
false-negatives when multiple valid paths exist — the field's answer is grade
**critical checkpoints + final state**, not full-sequence replay, reserving
strict order only where order is safety-critical. Deferred until fs+db state
oracles land (needs a new arg-**subset** matcher; all current QM matchers are
strict full-key-set).

*Caveats flagged by research: pass^k exact LaTeX secondary-sourced; some vendor
"trajectory eval" is LLM-judge under the hood; several MCP benchmarks are
preprints/workshop papers with small executed-task counts.*

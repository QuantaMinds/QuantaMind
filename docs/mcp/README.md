# MCP client integration — roadmap

This folder documents QuantaMind's Model Context Protocol (MCP) **client**: the
first runtime path where a local model executes real, side-effecting tools
(everything under `inference/eval/*` only *scores* tool emission against mocked
results). One markdown file per phase; steps are `## Step N` sections inside.

**Transport:** hand-rolled stdio JSON-RPC 2.0 (no new crates). **Types are
modeled from real captured bytes** in `fixtures/mcp/`, not from the spec —
MCP servers routinely violate their own spec, so the wire is the source of truth.

**Validation is two-track** (grade the world, not the words) — see
[methodology.md](methodology.md) for the τ-bench / pass^k / attribution grounding.

## Method / discipline

Every step follows the mandatory loop (`CLAUDE.md` rules 1, 2, 6): spec in this
doc → minimum code → test passes → inspect output → **live-run against the real
filesystem MCP server** (and, from Phase 5, a real llama.cpp model) → update docs →
commit. A green test never substitutes for inspecting real output.

## Phases

| Phase | File | Status |
|---|---|---|
| 0 — Fixtures from a real server | `../../fixtures/mcp/README.md` | ✅ done |
| 1 — Wire types from captured bytes | [phase-1-wire-types.md](phase-1-wire-types.md) | ✅ done |
| 2 — stdio transport + minimal client | [phase-2-transport.md](phase-2-transport.md) | ✅ done |
| 3 — Lifecycle hardening | [phase-3-lifecycle.md](phase-3-lifecycle.md) | ✅ done |
| 4 — Registry, secrets, security gate | [phase-4-registry-security.md](phase-4-registry-security.md) | ✅ done |
| 5 — Bridge MCP tools → local model | [phase-5-model-bridge.md](phase-5-model-bridge.md) | ✅ done (llama.cpp + llama.cpp live) |
| 6 — Schema oracle (Track A) | [phase-6-schema-oracle.md](phase-6-schema-oracle.md) | ✅ done |
| 7 — Error + attribution oracle (Track A) | [phase-7-attribution.md](phase-7-attribution.md) | ✅ done — **Track A complete** |
| 8 — World-manager (Track B) | [phase-8-world-manager.md](phase-8-world-manager.md) | ✅ done |
| 9 — Multi-turn execution + approval gate | [phase-9-agent-approval.md](phase-9-agent-approval.md) | ✅ done |
| 10 — FS-state oracle + pass^k scoring (Track B) | [phase-10-fs-oracle-scoring.md](phase-10-fs-oracle-scoring.md) | ✅ done — **Track B verdict + fake-done detection** |
| 11 — DB world (sqlite) | [phase-11-db-world.md](phase-11-db-world.md) | ✅ done |
| 12 — Test-page MCP path (UI) | [phase-12-ui.md](phase-12-ui.md) | 🔨 connect + two-track **mounted in COLLECTIONS**; builder/trace/verdict remain |

## Key invariants carried through every phase

- **Untrusted server output** (OWASP-LLM invariant e): tool descriptions,
  `instructions`, and results are rendered inert, never executed.
- **No machine info leaves** (invariant f): server stderr/args pass through
  `redact_path` before any log.
- **Path confinement** (invariant b): server roots go through
  `fs_guard::ensure_within` — the exact control the EscapeRoute CVEs broke.
- **Deny-by-default tool execution** (Phase 6): a side-effecting tool never runs
  without an explicit approval; the backend enforces this regardless of UI.

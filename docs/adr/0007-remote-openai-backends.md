# 0007 — Remote OpenAI-compatible backends (vLLM, SGLang) are a deliberate local-first exception

- **Status:** Accepted
- **Date:** 2026-07-04

## Context

QuantaMind is local-first: every LLM backend so far — llama.cpp, the bundled
`llama-server`, and `vllm_lm.server` — runs on `localhost` and is (mostly)
app-managed, with a hardcoded `http://localhost:<port>` endpoint and no auth.

But GPU inference at useful sizes doesn't fit a laptop. To benchmark and run against
vLLM and SGLang we need to reach a **remote GPU** (initially a GCP L4, 24 GB). These
servers speak the same OpenAI `/v1/chat/completions` wire vLLM already uses, but live
off-box and are typically launched with `--api-key`. Nothing in the codebase could
express "a backend the app does not spawn, reached at a user-configured URL with a
bearer token."

## Decision

Add `BackendKind::VLlm` and `BackendKind::SgLang` as **remote** backends that break
the local-first assumption on purpose:

- Their endpoint (URL) and bearer key come from `UserSettings`, pushed into a
  process-global `inference/backend/remote_config.rs` and read by
  `endpoint::resolve` — which returns `{ url, api_key }` and **errors clearly when
  the URL is unset** ("set it in Settings"), rather than reaching an empty/opaque host.
- They reuse the shared `inference/openai/` SSE codec (generation) and
  `openai::chat_tools` (native tool-calls), with an optional `Authorization: Bearer`.
- The app **never spawns or reaps** them: no `*ServerState`, no readiness/port/
  ownership guards, no `app_lifecycle` entry. Health and model discovery are plain
  `GET /v1/models` calls (`commands/remote/`).

## Consequences

- A remote backend is one thin adapter over the shared codec plus a settings-backed
  endpoint — no new lifecycle machinery. `model.backend` stays absolute and is
  server-sourced (`/v1/models`), so it never collides with vLLM's disk discovery.
- The app can now send prompts to a user-controlled remote host. This is opt-in
  (empty by default) and gated in the UI on a health probe, but it is a real widening
  of the trust/network boundary — hence this ADR records it explicitly.
- Guardrail: `remote_config` trims blank Settings fields to `None` so an unconfigured
  backend fails closed; `endpoint::resolve` unit-tests assert the unconfigured→error
  and configured→`{url, api_key}` behavior.

## Alternatives considered

- **SSH-tunnel to localhost + reuse the fixed-port pattern:** no settings/auth
  plumbing, but forces every user to run a tunnel and can't carry a per-server key —
  rejected as the default; a tunnel still works (just point the URL at `localhost`).
- **One shared remote URL for both backends:** simpler settings, but vLLM and SGLang
  are distinct engines a user may run side by side (or on different hosts) — rejected
  in favor of a URL + key per backend.

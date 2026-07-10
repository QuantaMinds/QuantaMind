# security.md — QuantaMind

Security model for QuantaMind, a **single-user, local-first** desktop app (Tauri 2 +
Rust + React). This doc is the internal engineering reference. The **public**
responsible-disclosure policy lives in the root `SECURITY.md`, not here. Test scenarios for
these controls live in [`security-testing.md`](security-testing.md).

Scope note: QuantaMind is not an enterprise multi-tenant service. The threat model below is
deliberately scoped to a single user on their own machine plus one optional, opt-in publish
path. Enterprise controls (real encryption-at-rest, tamper-evident audit logs, SOC 2
evidence, IPC role gates) are explicitly **out of scope** for this product — the seams exist
(`Cipher`/`Vault`, `AuditSink`) as no-ops so the separate enterprise product can implement
them without a redo, but this repo does not build them.

## invariants

The rule-7 invariants from `CLAUDE.md`. Every change must uphold all of them:

1. **Secrets via the port.** All secrets (cloud API keys, OAuth tokens) go through the
   `SecureSecrets` port backed by the OS keychain (`keyring`). Never plaintext on disk.
2. **Paths via `fs_guard`.** Every command taking a frontend-supplied filesystem path routes
   through `fs_guard`: canonicalize the FULL path (including the final component, so symlinks
   can't escape), confine to an allowed root, size-cap reads.
3. **Restrictive CSP.** The webview CSP stays tight (`default-src 'self'`). No new webview
   plugin scope (`fs`/`http`/`shell`) is added without an explicit review note here.
4. **HTTPS for credentials.** Any request carrying a credential is `https`-only; loopback
   (`127.0.0.1`/`localhost`) is exempt. Enforced at the `inference/http/http.rs` seam.
5. **Model output is untrusted (OWASP LLM Top-10).** Render model output as inert, escaped
   text — never `innerHTML`/`dangerouslySetInnerHTML`/`eval`. Treat file/web content the
   model reads as an indirect-prompt-injection source. The **Category K** eval axis
   *measures* a served config against exactly this: prompt-injection resistance and unsafe
   tool-call refusal, delivered as an indirect injection inside a tool result. A boundary
   failure is attributed to the model (followed the injection) vs the config (the served
   window silently evicted the safety guard) — see `docs/reference.md` Category K. This is
   an eval, not a runtime guard: it does not sanitize model output, and its verdict covers a
   fixed, known-injection set only (never a guarantee against adaptive attacks).
6. **No local machine info leaves the machine.** No absolute path or username appears in any
   log, error body, or publish payload. Paths pass through `redact_path` (`backend/src/redact.rs`)
   at the log/error/publish boundaries, and the publish payload is a *proven* field allowlist
   (`row_tests.rs` asserts no username survives + no un-allowlisted field ships). Note: the
   agentic transcripts do NOT capture the OS environment — their "env" is the SIMULATED task
   sandbox (scenario-defined file trees / web corpus / UI state), so there is no
   username/hostname/env-var there to redact.
7. **Publish/telemetry is opt-in + disclosed.** Nothing leaves the machine without an
   explicit user action, and the exact payload is shown before it is sent.

## trust-boundaries

- **Webview ↔ Rust (the primary boundary).** The React webview is treated as untrusted (any
  future XSS = attacker-in-webview). It cannot touch disk or network directly: there is NO
  `tauri-plugin-fs` and NO `tauri-plugin-http`. All file/network access is funneled through
  typed `#[tauri::command]` functions that validate at the boundary. `shell` is limited to a
  URL-scoped `shell:allow-open` allowlist. A restrictive CSP (`default-src 'self'`,
  `script-src 'self'`, no remote origins) is the backstop — set in `tauri.conf.json` with a
  relaxed `devCsp` for Vite HMR. There is no remote font and no CDN script: the Inter font is
  dropped in favour of the native system stack, and the Monaco editor is **self-hosted**
  (`frontend/src/shared/monacoSetup.ts` bundles `monaco-editor` + its worker locally instead
  of `@monaco-editor/react`'s default jsDelivr load), so the app ships zero runtime remote code
  and works fully offline.
- **Sidecars (loopback).** Ollama (`:11434`), llama-server (`:8081`), MLX (`:8082/8083`),
  whisper-server (`:8093`) run unauthenticated on `127.0.0.1`. They are reachable by any local
  process while running (standard local-LLM model). These are external processes we don't own,
  so they legitimately speak `http` on loopback — a blanket `https`-only client would break
  them; the credential guard (rule 7d) is instead scoped to requests that CARRY a key. The one
  listener the app itself runs — the OAuth callback — validates the `Host` header is loopback
  (DNS-rebinding defense, cf. Ollama CVE-2024-28224). The MLX locator prefers the configured
  path, then known-safe install dirs, and only then `$PATH` (with a warning), to blunt
  PATH-poisoning.
- **Publish (`api.quantamind.co`) — the only exfiltration path.** OAuth2 + PKCE + a `state`
  nonce (CSRF), bearer token; the callback is rejected on a `Host`/`state` mismatch. Payload is
  a strict field allowlist (`persistence/publish/row.rs`) with "no task content ever" and no
  machine identifiers (test-proven). Opt-in; payload shown pre-send.
- **Downloaded artifacts.** Model weights from HuggingFace over TLS + `verify_digest`. The app
  updater is minisign-signature-verified over HTTPS.

## secret-inventory

| Secret | Storage | Notes |
|---|---|---|
| OAuth refresh token | OS keychain (`keyring`), mem fallback | service `quantamind`, user `publish-refresh` |
| OAuth access token | in-memory only (`AuthState`) | never persisted, never logged |
| Cloud API keys (vLLM/SGLang) | OS keychain via `SecureSecrets` | migrated off plaintext YAML; `https`-only in transit |
| Updater public key | `tauri.conf.json` (minisign **public** key) | expected to be public |

No hardcoded secrets anywhere in the repo (CI secret-scans over history enforce this).

## enterprise-seams

These are laid as no-op ports in the OSS build so the separate enterprise product can attach
without a call-site redo. They are deliberately unimplemented here (see the scope note above).

- **Encryption at rest** — `persistence::at_rest` (`AtRest` trait; `Passthrough` no-op).
  Attach point: `persistence::jobs::transcripts::append_record` seals each appended JSONL line
  via `seal`; the reader opens via `open`. Because the transcript is append-only JSONL, the
  enterprise cipher must be a PER-LINE AEAD and the reader applies `open` per line. Extend the
  same trait to the history/readiness funnels if at-rest coverage must widen.
- **Audit log** — `crate::audit` (`AuditSink` trait; `NoopAudit` no-op; free `audit::record`).
  Emit points wired today: publish success (`publish_cmd::publish_to_board`), OAuth sign-in
  (`login_cmd::start_login`), and settings change (`settings::user_settings::set_user_settings`).
  The enterprise sink writes a tamper-evident (hash-chained / WORM) record; add emit calls at
  any new security-relevant event.

## threat-model

In scope (single-user local): a malicious website driving a local sidecar via the browser
(DNS-rebinding); a malicious eval collection / GGUF / workspace file (untrusted-file surface,
cf. CVE-2024-37032 path-traversal, CVE-2024-39720 malicious-GGUF DoS); on-path theft of a
cloud API key sent to a remote GPU; and — the user's top concern — **leakage of local machine
identity (paths/username/hostname/env) off the machine via logs, errors, transcripts, or the
publish payload**. Defenses: `fs_guard`, restrictive CSP, `https`-only credentials,
`SecureSecrets`, `redact_path` at the log/error/publish boundaries, and a proven publish
allowlist. (The transcript "env snapshot" is the simulated task sandbox, not the OS
environment — verified against `env_view.rs` — so it is not a machine-identity surface.)

Out of scope: a local attacker who already has the user's disk/keychain (they've already won);
multi-user/enterprise tenancy; nation-state supply-chain compromise of upstream deps (mitigated,
not eliminated, by `cargo audit`/`npm audit` + SBOM).

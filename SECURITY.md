# Security Policy

QuantaMind is a **single-user, local-first** desktop app (Tauri 2 + Rust + React). It runs
models on your own machine; the only data that can leave the machine is an explicit, opt-in
publish to the community leaderboard, whose payload is a metrics-only allowlist. The internal
engineering security model lives in [`docs/security.md`](docs/security.md).

## Supported versions

QuantaMind is pre-1.0. Security fixes land on the latest release and `main`. There is no
back-porting to older tags during the beta.

## Reporting a vulnerability

**Please do not open a public issue for a security vulnerability.** Instead, use one of:

1. **GitHub Private Vulnerability Reporting** (preferred) — the **Report a vulnerability**
   button under the repository's **Security** tab. This opens a private advisory only the
   maintainers can see.
2. **Email** — `info@quantamind.co` with subject `SECURITY`. Encrypt if you can; otherwise
   send a minimal report and we'll set up a secure channel.

Please include: the affected version/commit, a description of the issue and its impact, and
concrete reproduction steps or a proof of concept. If it involves the publish path or any way
local machine data could leave the machine, say so — that is our highest-priority class.

## What to expect

- **Acknowledgement** within 3 business days.
- An initial **assessment** (severity + whether we can reproduce) within 10 business days.
- We'll keep you updated on remediation and coordinate a disclosure timeline. We aim to fix
  high-severity issues before public disclosure, and we're happy to credit you (or keep you
  anonymous — your choice).

## Scope

In scope: the desktop app (Rust backend, React frontend), the publish client, the bundled
sidecar integration, and the update mechanism.

Out of scope: vulnerabilities in upstream dependencies already tracked by their own advisories
(report those upstream; we monitor via `cargo audit` / `pnpm audit`), issues that require a
local attacker who already has your OS user account or disk (they've already won a single-user
tool), and the security of third-party model weights you choose to download.

## Good-faith safe harbor

We will not pursue or support legal action against researchers who act in good faith, avoid
privacy violations and data destruction, and give us a reasonable chance to fix an issue
before disclosing it.

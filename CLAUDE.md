# CLAUDE.md — QM-Dev (QuantaMind)

Project guide for Claude Code sessions. Read this top-to-bottom before any work.

## Non-negotiable rules

1. **One step at a time.** Do not start the next step until the current one is
   (a) implemented, (b) its test case passes, AND (c) output verified for data
   quality. See `docs/process.md#workflow`.
2. **Test pass ≠ data quality pass.** A green test only proves the code ran the
   path you told it to. You must also inspect the *output* and confirm it
   matches the expected shape/values. See `docs/process.md#data-quality`.
3. **Separation of concerns + single-module architecture.** Each file does
   exactly one thing. Each module owns one responsibility. No "utils.ts"
   junk drawers. Split files by *responsibility*, not by line count. See
   `docs/architecture.md#architecture`.
4. **Documentation is part of the change.** When you change behavior, update
   the relevant section under `docs/` in the same commit.
5. **Locked tech stack.** Do not substitute libraries. See `docs/process.md#tech-stack`.
   Alternatives go to `docs/process.md#future-considerations`, never into code.
6. **Test LIVE against a real Ollama model — every time.** Unit tests / `tsc` /
   vitest are necessary but never sufficient. After they pass, run the change
   against an actual model from Ollama (`:11434`) and inspect the real output.
   This project's hardest bugs (empty `.` output, foreign-dialect soup, the
   native-path discard, read_file acking empty) surfaced ONLY by running the live
   model — a green test just proves the path you told it to run.
7. **Security invariants (never violate).** See `docs/security.md#invariants`.
   (a) Secrets go ONLY through the `SecureSecrets` port (OS keychain), never
   plaintext on disk. (b) Every command that takes a filesystem path from the
   frontend goes through `fs_guard` (canonicalize the FULL path, confine to an
   allowed root). (c) The webview CSP stays restrictive — no new webview plugin
   scope (`fs`/`http`/`shell`) without an explicit review note. (d) Credentialed
   HTTP is `https`-only (loopback exempt). (e) Treat ALL model output as untrusted
   (OWASP LLM) — render inert, never `innerHTML`/execute. (f) **No local machine
   info leaves the machine** — no absolute path or username in any log, error
   body, or publish payload; paths pass through `redact_path`, and anything
   published is a *proven* field allowlist (test-enforced). (g) Publish/telemetry
   is strictly opt-in and disclosed.

## Workflow per step (mandatory loop)

```
1. Read the step spec (from docs/process.md#phase-roadmap or user request)
2. Write the minimum code to satisfy it
3. Write the test for the expected behavior
4. Run the test — must pass
5. Inspect actual output vs expected output — must match in shape AND value
6. Run it LIVE against a real Ollama model and inspect the real output (rule 6)
7. Update the relevant section(s) under docs/
8. Commit (Conventional Commits)
9. Only now: move to the next step
```

If step 5 or 6 fails, do not "fix" by loosening the assertion. Fix the code.

## Quick reference

- **Stack:** Tauri 2.x + Rust + React 19 + TS 5 + Vite + Tailwind + Zustand.
  Full table in `docs/process.md#tech-stack`.
- **Layout:** `frontend/src/` (React features) + `frontend/` configs,
  `backend/src/` (Rust commands + inference) + `backend/tauri.conf.json`,
  plus `docs/`. Full tree in `docs/architecture.md#folder-structure`.
- **Naming:** Rust `snake_case`, TS `camelCase`, components `PascalCase`,
  branches `<type>/<short-description>` (`feature/`, `fix/`, `bug/`, `docs/`,
  `chore/`, `refactor/`) — name the change, not a phase. Full list in
  `docs/process.md#conventions`.
- **Setup:** Day-zero install + verification steps in `docs/process.md#setup`.
- **Phases:** Current roadmap and phase boundaries in
  `docs/process.md#phase-roadmap`.

## What NOT to do

- Do not add a logging library, state-machine library, UI kit, form library,
  or extra Tauri plugins until the phase that requires them.
- Do not create `utils/`, `helpers/`, `common/`, or `misc/` directories.
- Do not write multi-paragraph comments. Code is self-documenting; docs live
  in `docs/`.
- Do not skip a test because "it's obvious." Obvious code still fails.
- Do not refactor opportunistically during a feature commit. Refactor in a
  separate commit with its own tests.
- Do not let a file take on a second responsibility. Split by concern.

## When in doubt

- File doing two things → split by concern (not by line count).
- Test passing but output looks wrong → trust the output, fix the test or
  the code.
- Doc out of date → update the doc before the next commit.
- Architectural question → propose in chat, do not invent silently.

## Docs

Engineering docs live in four files under `docs/` (link with anchors, e.g.
`docs/architecture.md#layering`):

- **`architecture.md`** — `#architecture` (modules + IPC), `#layering`
  (dependency law + sink/thin-command), `#robustness` (no silent failures /
  no leaky data), `#folder-taxonomy` (≤10 files per folder), `#folder-structure`.
- **`process.md`** — `#tech-stack` (locked deps), `#setup` (day-zero),
  `#conventions` (naming/commits/branches), `#workflow` (the step loop),
  `#data-quality` (verify output), `#phase-roadmap`, `#future-considerations`.
- **`reference.md`** — `#analysis-schema` (bench/analysis contract),
  `#troubleshooting` (error-state help; anchors back the in-app links).
- **`security.md`** — `#invariants` (the rule-7 security invariants), `#trust-boundaries`
  (webview↔Rust, sidecars, publish), `#secret-inventory`, `#threat-model` (scoped to the
  single-user local model). Public disclosure policy lives in root `SECURITY.md`, not here.

`docs/prompts/` is a runtime app asset (bundled prompt templates), not docs.

Keep this file lean. If it grows, move detail into `docs/`.

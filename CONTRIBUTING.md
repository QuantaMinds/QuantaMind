# Contributing to QuantaMind

Thanks for helping build QuantaMind. This guide keeps the repo clean, reviewable,
and pleasant to maintain. Read it once before your first PR — it takes five
minutes and saves everyone hours.

The short version:

> **One change per PR. Branch off `main`. Tests pass *and* output is verified.
> Docs ship in the same commit. Conventional Commit messages. Keep `main` green.**

---

## Table of contents

- [Ground rules (read first)](#ground-rules-read-first)
- [Project setup](#project-setup)
- [The workflow loop](#the-workflow-loop)
- [What to do / what NOT to do](#what-to-do--what-not-to-do)
- [Branching](#branching)
- [Commit messages](#commit-messages)
- [Code style](#code-style)
- [Testing & the data-quality gate](#testing--the-data-quality-gate)
- [How to raise a PR](#how-to-raise-a-pr)
- [PR checklist](#pr-checklist)
- [Code review etiquette](#code-review-etiquette)
- [Reporting bugs & requesting features](#reporting-bugs--requesting-features)
- [Security](#security)

---

## Ground rules (read first)

These are non-negotiable and come from [`CLAUDE.md`](./CLAUDE.md) and
[`docs/process.md`](./docs/process.md). A PR that violates them will be asked to
change before review.

1. **One step at a time.** Don't start the next change until the current one is
   implemented, tested, and its output verified.
2. **A green test is necessary, not sufficient.** Tests prove the code ran the
   path you wrote; you must also inspect the *output* and confirm shape + values.
   See the [data-quality gate](#testing--the-data-quality-gate).
3. **Separation of concerns.** Each file does exactly one thing. Split by
   *responsibility*, not by line count. No `utils.ts`, `helpers/`, `common/`, or
   `misc/` junk drawers. (Folder taxonomy: ≤ 10 files per folder.)
4. **Documentation ships with the change.** If you change behavior, update the
   relevant doc under `docs/` (and the in-app **Help** page if it's user-facing)
   in the *same* commit.
5. **The tech stack is locked.** Tauri 2 · Rust · React 18 · TS 5 · Vite ·
   Tailwind · Zustand. Don't add a logging/state-machine/UI-kit/form library or
   a new Tauri plugin without explicit review. Alternatives go to
   `docs/process.md#future-considerations`, never silently into code.

If something here conflicts with `CLAUDE.md`, **`CLAUDE.md` wins** — tell us so we
can fix the doc.

---

## Project setup

You only need this to **build QuantaMind from source** — to *use* it, grab the
prebuilt app or `qm` CLI from the [README Quick start](./README.md#-quick-start-no-build-required).

macOS is first-class today; Windows and Linux dev builds run too, with sidecar
lifecycles being rewired phase-by-phase (see the README [Roadmap](./README.md#roadmap)).
The whole setup takes ~5 minutes on macOS.

**1 · Install toolchains** (skip any you already have)

<table>
<tr><th>macOS</th><th>Linux (Debian / Ubuntu)</th><th>Windows</th></tr>
<tr valign="top"><td>

```bash
brew install rust node pnpm llama_cpp
xcode-select --install
```

</td><td>

```bash
# Tauri system deps
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev build-essential \
  curl wget file libxdo-dev libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev patchelf
# Rust, Node 20+, pnpm, llama.cpp
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash - \
  && sudo apt install -y nodejs
corepack enable pnpm
# llama.cpp: build or download a release from https://github.com/ggml-org/llama.cpp
```

</td><td>

```powershell
winget install Rustlang.Rustup OpenJS.NodeJS `
  pnpm.pnpm Microsoft.VisualStudio.2022.BuildTools
# llama.cpp: grab a release from
# https://github.com/ggml-org/llama.cpp/releases
# Add "Desktop development with C++" in the
# VS installer (MSVC linker). WebView2 ships
# with Windows 11. Then see the "Windows dev
# shell" note below to source cargo + MSVC.
```

</td></tr></table>

**2 · Start llama.cpp on a small GGUF**

```bash
# --jinja is required — without it generations loop instead of stopping.
llama-server -m ./models/Llama-3.2-1B-Instruct-Q4_K_M.gguf \
  --host 127.0.0.1 --port 8081 --jinja -c 8192 &
```

**3 · Clone, install, run**

```bash
git clone https://github.com/QuantaMinds/QuantaMind.git
cd QuantaMind/frontend
pnpm install
pnpm tauri dev            # first build is slow; opens a native window
pnpm tauri build          # production → backend/target/release/bundle/ (.dmg + .app)
```

<details>
<summary><b>Prerequisites & optional backends</b></summary>

| Tool | Version | Required? |
|---|---|---|
| **Rust** | 1.75+ | required |
| **Node** | 20+ | required |
| **pnpm** | 9+ | required |
| **llama.cpp** | latest | required — the default backend |
| **llama.cpp** (`llama-server`) | latest | optional — run GGUF models directly |
| **vLLM** (`pip install vllm-lm`) | latest | optional — Apple Silicon only |
| **vLLM** / **vLLM** | latest | optional — a **remote** OpenAI-compatible GPU server; set its URL (+ `--api-key`) in Settings |

</details>

<details>
<summary><b>Windows dev shell</b> — one-time setup for <code>pnpm tauri dev</code></summary>

On Windows, source two things into your PowerShell session before `cargo` (and therefore `pnpm tauri dev`) can run: **the Rust bin dir on PATH** (rustup only updates *new* shells) and the **MSVC linker env** from Visual Studio's `vcvars64.bat`. macOS and Linux need neither.

If you hit `program not found: cargo metadata` or `linker link.exe not found`:

```powershell
# Refresh PATH for this session (already permanent on the User env var).
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:APPDATA\npm;$env:LOCALAPPDATA\Programs\llama.cpp;$env:Path"

# Source MSVC env into PowerShell. Adjust the path for your VS version
# (2022\BuildTools, 2022\Community, or 18\Community for VS 2026).
$vcvars = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
cmd /c "call `"$vcvars`" && set" |
  Where-Object { $_ -match "^(INCLUDE|LIB|Path|LIBPATH)=" } |
  ForEach-Object { $n,$v = $_ -split "=", 2; Set-Item -Path "env:$n" -Value $v }

cargo --version; link.exe /?      # both should print without error
cd path\to\QuantaMind\frontend
pnpm tauri dev
```

**pnpm 11 quirk (`ERR_PNPM_IGNORED_BUILDS: esbuild@…`):** the repo's `pnpm-workspace.yaml` pre-approves `esbuild`, so a fresh clone Just Works. On a stale worktree, re-pull `main`. Save the block above as `run-dev.ps1` if you spin up dev shells often.

</details>

Before pushing, the same checks CI runs:

```bash
# frontend
cd frontend && pnpm test && npx tsc --noEmit
# backend
cd backend && cargo test && cargo clippy --tests && cargo fmt --check
```

---

## The workflow loop

Every change follows the same loop (from `docs/process.md#workflow`):

```
1. Understand the step
2. Implement the minimum that satisfies it
3. Write the test for the expected behavior
4. Run the test — it must pass
5. Verify the actual output vs expected (data-quality gate)
6. Update the relevant docs
7. Commit (Conventional Commits)
8. Only now move to the next step
```

If step 5 fails, **fix the code — never loosen the assertion.**

---

## What to do / what NOT to do

### ✅ Do

- Keep each PR to **one logical change**. Small PRs get reviewed fast.
- Write the **test first or alongside** the code, named after the behavior
  (`streams_tokens_in_order`, not `test_run_prompt`). One behavior per test.
- **Verify output**, not just that tests are green (shape, values, units,
  edge cases, Rust→JSON→TS field round-trip).
- Keep files **single-concern**; split when a file starts doing two things.
- Update **docs in the same commit** as the behavior change.
- Use **typed errors** end to end (`Result<T, AppError>` in Rust; discriminated
  unions across IPC in TS).
- Rebase or merge `main` into your branch before requesting review so it merges
  cleanly.

### ❌ Don't

- **Don't stack steps** ("I'll knock out 1–3 then test"). One step, tested, then
  the next.
- **Don't loosen a failing assertion** to make it pass. Fix the code.
- **Don't skip verification because the test passed.**
- **Don't refactor opportunistically inside a feature PR.** Refactors are their
  own branch + PR with their own tests.
- **Don't create `utils/`, `helpers/`, `common/`, `misc/`** or let a file take on
  a second responsibility.
- **Don't add dependencies or swap libraries** without review (locked stack).
- **Don't commit secrets, large binaries, or generated artifacts.** Check
  `.gitignore` first.
- **Don't `unwrap()` outside tests** (Clippy denies it in critical files).
- **Don't push directly to `main`.** Always branch + PR.
- **Don't bundle unrelated changes** ("while I was in there…") into one PR.

---

## Branching

- Branch off the latest `main`.
- Name branches `<type>/<short-description>` (kebab-case), where `<type>` names the
  change: **`feature/`** (new behavior), **`fix/`** (correctness fix), **`bug/`** (a
  reported bug). For non-code changes use `docs/`, `chore/`, or `refactor/` (matching
  the commit types). Examples: `feature/streaming-output`,
  `fix/native-budget-truncation`, `bug/empty-output-verdict`.
- One branch = one PR = one logical change.
- Delete the branch after the PR merges.

---

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/). The prefix sets the
intent:

| Prefix | Use for |
|---|---|
| `feat:` | New user-visible behavior |
| `fix:` | Bug fix |
| `docs:` | Documentation only |
| `test:` | Adding or fixing tests |
| `chore:` | Tooling, deps, config |
| `refactor:` | No behavior change |

- Format: `type(scope): summary` — e.g. `feat(eval): native function-calling column`.
- Summary in the imperative, lower-case, no trailing period, ≤ ~72 chars.
- **One step = one commit** (or a tight, related series). Don't mix a feature and
  an unrelated fix in one commit.
- The PR **title** follows the same convention; the body references `closes #N`
  when it resolves an issue.

---

## Code style

Full table in [`docs/process.md#conventions`](./docs/process.md#conventions).

| Domain | Style | Example |
|---|---|---|
| Rust fn / vars | `snake_case` | `run_prompt` |
| Rust types | `PascalCase` | `InferenceBackend` |
| Rust constants | `SCREAMING_SNAKE` | `DEFAULT_TIMEOUT_MS` |
| TS fn / vars | `camelCase` | `runPrompt` |
| TS components / types | `PascalCase` | `PromptEditor` |
| React component file | `PascalCase.tsx` | `PromptEditor.tsx` |
| TS non-component file | `kebab-case.ts` | `use-streaming-run.ts` |
| Rust file | `snake_case.rs` | `llama_cpp.rs` |
| Branch | `<type>/<short-description>` | `feature/streaming-output`, `fix/native-budget` |

- **Comments:** default to none — naming and structure carry meaning. Write a
  one-line comment only when the *why* is non-obvious (a workaround, an invariant,
  a constraint). No multi-paragraph essays; no ticket/caller references.
- **Errors:** Rust `Result<T, AppError>`, no `unwrap()` outside tests; TS returns
  discriminated unions across IPC — never throw across the IPC boundary.
- **Imports:** absolute for cross-feature TS (`@/shared/...`), relative within a
  feature; Rust prefers `use crate::module::...`.
- **Colors:** flow through theme tokens in `frontend/src/styles/tokens.css`. No
  hardcoded hex outside that file.
- **No fabricated numbers.** If a value isn't measured, show "Not available" —
  never a derived/estimated stand-in presented as real.

---

## Testing & the data-quality gate

```bash
# frontend
cd frontend && pnpm test
# backend
cd backend && cargo test
```

Tests live next to the code (`__tests__/` in TS, inline `#[cfg(test)]` in Rust,
integration in `backend/tests/`).

After a green test, run the **data-quality gate** before you call it done:

| Check | Looking for |
|---|---|
| **Shape** | Correct types, required fields present, no surprise fields; streams: chunk count, ordering, terminator |
| **Values** | Reasonable ranges; correct units (ms vs s, bytes vs MB); correct encoding (UTF-8, no BOM) |
| **Edge cases** | Empty input → empty output (not crash); large input handled/rejected; Unicode/emoji/RTL preserved; malformed input → typed error, not panic |
| **Cross-boundary** | Rust → JSON → TS field round-trip (snake_case ↔ camelCase!); disk → memory YAML byte-identical |
| **Determinism** | Same input → same output where applicable |

If it fails, **fix the code, not the assertion.**

---

## How to raise a PR

1. **Sync `main`** and branch:
   ```bash
   git switch main && git pull
   git switch -c feature/your-change   # or fix/… , bug/… , docs/… , chore/…
   ```
2. **Make the change** following the [workflow loop](#the-workflow-loop) — small,
   focused, tested, docs updated.
3. **Run the full local checks** (tests + typecheck + clippy + fmt) — see
   [Project setup](#project-setup). Green locally before you push.
4. **Push** and open the PR against `main`:
   ```bash
   git push -u origin feature/your-change
   gh pr create --base main --fill   # or open it in the GitHub UI
   ```
5. **Write a useful PR description:** what changed, why, how you verified it
   (paste test output / screenshots for UI), and `closes #N` for any issue.
6. **Keep it small.** If the diff is sprawling, split it into stacked PRs.
7. **Wait for CI to go green**, then request review. Don't merge your own PR
   without a review unless you're the sole maintainer and CI is green.
8. **Respond to review** by pushing follow-up commits (don't force-push during an
   active review — it makes re-review harder). Squash on merge keeps history clean.
9. **After merge**, delete the branch.

---

## PR checklist

Copy this into your PR description and tick it off:

```md
- [ ] One logical change; unrelated edits removed
- [ ] Branched off the latest `main`
- [ ] Tests added/updated and passing locally (frontend + backend)
- [ ] Output verified against the data-quality gate (not just green tests)
- [ ] `tsc --noEmit`, `cargo clippy --tests`, `cargo fmt --check` clean
- [ ] Docs updated in the same PR (docs/ and/or in-app Help)
- [ ] Conventional Commit messages; PR title follows the convention
- [ ] No new dependencies / library swaps (or called out + justified)
- [ ] No secrets, large binaries, or generated artifacts committed
- [ ] CHANGELOG updated if user-visible
- [ ] Linked the issue (`closes #N`) where applicable
```

---

## Code review etiquette

**As an author:** keep PRs small, describe *why*, and make CI green before asking
for review. Treat review comments as a conversation, not a verdict.

**As a reviewer:** review promptly, be specific and kind, distinguish blocking
issues from nits (prefix nits with `nit:`), and approve when it's good enough —
not perfect. Verify the change does what it claims, not just that it compiles.

Merge only when: CI is green, at least one approval (for shared repos), the branch
is up to date with `main`, and the PR is one coherent change.

---

## Reporting bugs & requesting features

Open a [GitHub issue](https://github.com/QuantaMinds/QuantaMind/issues). For bugs,
include: what you did, what you expected, what happened, your OS + app version
(see **Help** tab), and the model/backend in use. Minimal reproduction steps beat
a long description.

For features, describe the problem you're trying to solve before the solution you
have in mind.

---

## Security

Do **not** open a public issue for a vulnerability. Open a
[private security advisory](https://github.com/QuantaMinds/QuantaMind/security/advisories/new)
instead — see [`SECURITY.md`](./SECURITY.md).

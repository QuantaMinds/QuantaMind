# Changelog

All notable changes to QuantaMind are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project aims to follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- **Release pipeline was down since 0.2.4**: the dist workflow chained the Docker build
  (#216) without granting the called workflow's permissions, so GitHub rejected the whole
  file at parse time — every run since Jul 23 (PRs *and* any future `quantamind-v*` tag)
  hit `startup_failure`. Fixed via `github-custom-job-permissions` in `dist-workspace.toml`
  + `dist generate` (the file stays machine-generated, per the release-pipeline rule).

### Added

- **Context Stress Test verdict honesty** (prompted by community review of a published run —
  credit: community review): every rung now carries an uncapped per-task breakdown
  (`by_task`; GUI table, `qm cliff` `failures:` lines, JSON); a collapse verdict requires the
  drop's Wilson/Newcombe interval to exclude zero on top of the 20pp margin (else the honest
  `Inconclusive`); and a collapse whose failures concentrate in one task is labeled
  **low confidence** (exact exchangeability p-value + leave-one-task-out check), on the live
  run, the persisted Matrix cell, and the CLI.
- **Cliff thinking budget** — Lean/Standard/Deep presets whose reasoning scratchpad scales with
  each rung's depth through the same per-tier table the Tests page uses; `qm cliff --thinking`.
- **Cliff decoding is params-first** — a globally set temperature is honored (and stamped on
  the report); greedy 0 stays the reproducible default.
- **Context Stress Test depth ceiling is now honest on llama.cpp** — the Max-Tokens slider
  caps at the *running* llama-server's launch window (state or `/props`, external launches
  included) with an inline raise-or-reduce hint, and `qm cliff` preflights the same window
  (`[QM-WINDOW-TOO-SMALL]`, exit `2`) instead of dying mid-ladder. This was the "stress test
  maxes out at ~9K no matter what I set" report.
- **Context Stress Test on Ollama: the native-FC ceiling shows before the click** — Ollama
  grants the `tools` capability only when a model's template references `.Tools` (imported
  GGUFs usually don't), and rejects native tool requests otherwise. The Method toggle now
  disables Native FC for such a model with a hint naming the cause and both levers (probe
  Prompt-based, or re-create the model with a tool-capable TEMPLATE) instead of refusing
  post-click on every run. This was the "Ollama doesn't work with the stress test" report.
- **Three-bucket cliff aggregate** — a cap-affected rung reports `passed · failed ·
  died-at-cap` and no single rate (dropping budget cells overstates, folding them
  understates); model claims run on the content rate, budget-event scale on the folded
  rate, and the invariant — budget-limited cells never enter a numerator or denominator
  that claims to measure the model — is documented for every future roll-up.
- **Deliberation Headroom** — the Context Stress Test measures budget consumption per cell
  (decoded tokens vs the cap in force, thinking tokens where measurable, cap-hit stop reasons):
  passing tasks within 150‰ of the cap get an amber "likely to fail deeper" warning; failures
  that died at the cap are marked; and a rung whose failures ALL died at the cap classifies
  **BudgetLimited** (exit `12`) — a budget-bound measurement, never a model collapse. No other
  eval harness distinguishes "failed because cut" from "failed on content".
- **`medium-coding-v3`** — the Medium coding collection grows 5 → 18 tasks in three deliberate
  complexity bands of six (shallow single-getter, medium binary-branch, deep conditional
  chains), so one task moves a Context Stress Test rung ~5.5pp instead of 20 and the
  depth×complexity interaction is measurable by design. v2 stays bundled (saved runs keep
  resolving); the pickers offer v3.

## [0.2.4] — 2026-07-23

CLI quality-of-life + eval-integrity release.

### Added

- **`--save-transcripts <dir>`** on `qm run`/`qm test` — every task's per-step trajectory (raw model output, injected tool results, timings) as JSONL, the same format the desktop app's trace store uses. Post-mortem failing runs straight from the CLI.
- **Verdict line now shows `avg steps`**; every `--costs` row shows **`kv@peak`** — that task's own KV-cache size at its peak context (f16 baseline; dims-gated, `n/a` never a guess).
- **Three new authoring validators** — prompt-leaked facts, cross-entity duplicate fields, faults on confirmation checkpoints — enforced everywhere a collection enters: `qm validate`, JSON import, MCP world validation, and the bundled-collection CI guards.
- **`medium-coding-v2`**: the Medium coding collection re-authored so every checkpoint sits on the information-necessary path (the v1 defects made some tasks structurally unpassable; v1 stays bundled so saved runs keep resolving).

### Changed

- Suite-wide task de-leak: 15 tasks across 9 collections no longer state in the prompt the facts their checkpoints require discovering; two hard-coding faults re-keyed off confirmation calls. Verdicts on these collections are now honest capability signals.
- The `qm-eval` GitHub Action defaults to `install: release` — prebuilt binary in seconds, no Rust toolchain (`install: source` remains, now building lean without GUI system deps).

### macOS note (unsigned builds)

Our macOS builds aren't Apple-signed yet. The **curl installer is unaffected** — use it. A browser-downloaded tarball gets quarantined and Gatekeeper blocks the binary; fix: `xattr -d com.apple.quarantine ./qm` (or right-click → Open). Integrity without Apple's stamp: every artifact is sha256-checksummed and carries a GitHub build attestation — `gh attestation verify <file> --owner QuantaMinds`.

## [0.2.3] — 2026-07-22

First release of the headless `qm` CLI as **prebuilt binaries** — no Rust
toolchain, no `cargo build`.

### Added

- **One-line install** (macOS/Linux): `curl -fsSL https://github.com/QuantaMinds/QuantaMind/releases/latest/download/quantamind-installer.sh | sh` — installs `qm` into `~/.local/bin`. Windows: `powershell -ExecutionPolicy Bypass -c "irm https://github.com/QuantaMinds/QuantaMind/releases/latest/download/quantamind-installer.ps1 | iex"`.
- Prebuilt `qm` for six targets: macOS (Apple Silicon + Intel), Linux glibc (x64 + arm64), Linux **static musl** (x64, runs in any container), Windows x64.
- Every artifact ships with sha256 checksums and GitHub artifact attestations (`gh attestation verify <file> --owner QuantaMinds`).

### Changed

- The CLI binary is now **headless by construction**: a `gui` cargo feature seam keeps the Tauri/desktop dependency tree (webkit/GTK/ALSA) out of `qm` entirely; CI enforces this with a linkage guard.
- HTTP now uses rustls with the OS trust store (no OpenSSL system dependency).

*(0.2.1/0.2.2 were desktop-only tag iterations without published releases.)*

## [0.2.0] — 2026-06-06

The first release since 0.1.0 — so these notes cover everything added since the
initial workbench: two more inference backends, and five new tabs that take you
from a single prompt all the way to a hardware-aware agent-readiness verdict.

**Platform:** macOS only for now (Windows/Linux planned).

### Added

**Backends**
- llama.cpp (`llama-server`) and MLX (`mlx_lm`, Apple Silicon) backends, alongside Ollama, behind a single `InferenceBackend` trait.
- Backend is auto-selected from the model's weight format (never a silent fallback).
- External servers launch stream-aware (no blind timeout), are reaped on app exit, and bind to a dynamically chosen free port.

**Inspector** — per-token timing forensics for a run
- TTFT breakdown (model-load vs prompt-prefill vs generation) as a stacked phase bar.
- Per-token latency timeline (visx) with outlier highlighting and phase boundaries.
- Inter-token latency histogram, VRAM bar, and context-budget bar.
- Hardware detection (CPU/cores/RAM/OS/arch), cold- vs warm-start comparison, memory-leak heuristic, regression alerts, and a self-contained HTML report export.

**Eval** — score models on tool use and agentic reliability
- Single-turn tool-calling: composite accuracy (parse · tool-selection · args · abstain) with cascaded conditional denominators; deterministic, sandbox-free scoring.
- Multi-step agentic: Pass^k reliability, average steps, effort (tokens on success), schema resilience, and a dominant-failure-mode breakdown.
- Context-cliff probe — finds the prompt length where tool-call accuracy collapses, plotted against real measured prompt tokens.
- Custom task collections by hand or via CSV import; optional native function-calling path (Ollama `/api/chat` `tools`); per-task trace debugger.

**Quant** — compare a model family's quantizations
- Side-by-side size · hardware fit (OOM risk) · quality (eval pass-rate) · tool-call composite, with a best-trade-off recommendation for your use case and context length.

**Agent Report** — go/no-go readiness validator
- Per-model **Ready / Conditional / Not Ready** verdict with the exact blocking and conditional reasons.
- Hardware-aware VRAM fit (exact weights + KV cache vs an allocation cap, with a pressure flag).
- Configurable readiness profiles (min Pass^k, forbid loops/false-done, require full VRAM, min context, require native FC).
- Resumable, crash-recoverable run queue; verdict-table HTML export.

**Analysis (Compare)**
- Throughput + TTFT comparison chart and a word-level output diff.

**Help**
- In-app documentation page: a left sidebar + center content documenting every page, tool, and graph as What it does / Why it exists / How it works — with inline formulas and source files for every computed metric.

**Daily-driver polish**
- Parameter controls, auto-rerun on save, prompt history, richer error states, light/system theme, onboarding, keyboard shortcuts, named workspaces, and in-app auto-update.

### Changed
- Supported platform scoped to macOS for now.
- License is Apache 2.0.

### Fixed
- Context-cliff verdict: a broken baseline (0% accuracy at the smallest context) now reads **"fails from start"** instead of a misleading **"✓ no cliff"** — the baseline must clear the pass bar before "no cliff" can be claimed.

## [0.1.0]

- Initial workbench: Workspace (prompt → streamed completion with timing), Model Management (install from Ollama library / Hugging Face GGUF / local file), and Compare (one prompt across multiple models). Local-first, no telemetry.

[0.2.0]: https://github.com/QuantaMinds/QuantaMind/releases/tag/v0.2.0
[0.1.0]: https://github.com/QuantaMinds/QuantaMind/releases/tag/v0.1.0

# Changelog

All notable changes to QuantaMind are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/), and the project aims to follow
[Semantic Versioning](https://semver.org/).

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

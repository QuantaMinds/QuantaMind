<div align="center">

# QuantaMind

**The pre-deployment gate for local AI agents.**

Benchmark any **Ollama**, **llama.cpp**, or **MLX** model for *agentic readiness* on your own hardware and get a **Ready / Conditional / NotReady** verdict — before you wire it into an agent. Nothing leaves the machine.

<sub>Local-first · No telemetry · No account · pass^k scoring · hardware-aware · one ~30 MB binary</sub>

<br/>

![Version](https://img.shields.io/badge/version-0.2.0-blue)
![Platform](https://img.shields.io/badge/platform-macOS-blue)
![Tauri](https://img.shields.io/badge/Tauri-2.x-FFC131?logo=tauri&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)
![License](https://img.shields.io/badge/license-Apache%202.0-green)
![Status](https://img.shields.io/badge/status-active%20development-yellow)

[![Repo](https://img.shields.io/badge/GitHub-QuantaMinds%2FQuantaMind-181717?logo=github)](https://github.com/QuantaMinds/QuantaMind)

</div>

---

## Table of contents

- [Quick start](#quick-start)
- [Your local model passed the demo. Will it survive an agent?](#your-local-model-passed-the-demo-will-it-survive-an-agent)
- [What it does](#what-it-does)
- [How it works](#how-it-works)
- [Why local, why pre-deployment](#why-local-why-pre-deployment)
- [Features](#features)
- [Roadmap](#roadmap)
- [Tech stack](#tech-stack)
- [Architecture](#architecture)
- [Building from source](#building-from-source)
- [Project layout](#project-layout)
- [Deep dive — Workspace](#deep-dive--workspace)
- [Deep dive — Speech-to-Text](#deep-dive--speech-to-text)
- [Deep dive — Model Management](#deep-dive--model-management)
- [Deep dive — Analysis](#deep-dive--analysis)
- [Install pipeline internals](#install-pipeline-internals)
- [Live model browsing](#live-model-browsing)
- [Engineering principles](#engineering-principles)
- [Development workflow](#development-workflow)
- [Testing](#testing)
- [Contributing](#contributing)
- [Security & privacy](#security--privacy)
- [FAQ](#faq)
- [License](#license)
- [Acknowledgements](#acknowledgements)

---

## Quick start

**Zero to a running window in ~5 minutes.** macOS is first-class today; **Windows and Linux support is landing in phases** (see [Roadmap](#roadmap)) — the platform-adapter foundation is in place, sidecar lifecycles are being rewired. There are no prebuilt downloads yet, so running from source is the way in.

```bash
# 1) Toolchains  (skip any you already have)
brew install rust node pnpm ollama
xcode-select --install

# 2) Start Ollama + pull a small model to gate
ollama serve &
ollama pull llama3.2:1b
curl http://localhost:11434/api/tags          # smoke-test: Ollama is up

# 3) Clone, install, run
git clone https://github.com/QuantaMinds/QuantaMind.git
cd QuantaMind/frontend
pnpm install
pnpm tauri dev                                 # first build is slow; opens a native window
```

That's it. The first run opens a native window. Open the **Tests** tab, pick the model you pulled, and run a built-in agentic collection to get your first **Ready / Conditional / NotReady** verdict — then check the **Agent Report** tab for the per-model breakdown. (Editing `frontend/src/App.tsx` and saving triggers HMR.)

> 💬 **Hit a snag in these steps? [Open an issue](https://github.com/QuantaMinds/QuantaMind/issues)** — frictionless setup is a goal, so setup bugs are real bugs.

<details>
<summary><b>Prerequisites</b> — versions, and the optional backends</summary>

| Tool | Version | Required? |
|---|---|---|
| **Rust** | 1.75+ | required |
| **Node** | 20+ | required |
| **pnpm** | 9+ | required |
| **Ollama** | latest | required — the default backend |
| **llama.cpp** (`llama-server`) | latest | optional — run GGUF models directly |
| **MLX** (`pip install mlx-lm`) | latest | optional — Apple Silicon only |
| **whisper.cpp** | latest | optional — speech-to-text; macOS `brew install whisper-cpp`, Windows/Linux direct download from [whisper.cpp releases](https://github.com/ggerganov/whisper.cpp/releases) |

</details>

<details>
<summary><b>Windows dev shell</b> — one-time environment setup for <code>pnpm tauri dev</code></summary>

On Windows you need two things sourced into your PowerShell session before `cargo` (and therefore `pnpm tauri dev`) can run: **the Rust bin dir on PATH** (rustup adds it to the *User* env var, so only shells started *after* rustup install pick it up), and the **MSVC linker + include/lib env** from Visual Studio's `vcvars64.bat`. macOS and Linux need neither.

If you hit `program not found: cargo metadata` or `linker link.exe not found`, paste this at the top of your shell:

```powershell
# Add Rust + pnpm + Ollama to PATH (already permanent on the User env var —
# this refreshes the current session so you don't have to reopen the shell).
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:APPDATA\npm;$env:LOCALAPPDATA\Programs\Ollama;$env:Path"

# Source MSVC env (link.exe, INCLUDE, LIB) into PowerShell. Adjust the path
# for VS 2022 (17.x) — the folder is `2022\BuildTools` or `2022\Community`
# instead of `18\Community` for VS 2026.
$vcvars = "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
cmd /c "call `"$vcvars`" && set" |
  Where-Object { $_ -match "^(INCLUDE|LIB|Path|LIBPATH)=" } |
  ForEach-Object {
    $n,$v = $_ -split "=", 2
    Set-Item -Path "env:$n" -Value $v
  }

# Sanity check — both should print without error.
cargo --version
link.exe /?

# Launch the app.
cd path\to\QuantaMind\frontend
pnpm tauri dev
```

**pnpm 11 quirk (`ERR_PNPM_IGNORED_BUILDS: esbuild@…`).** pnpm 11's supply-chain gate blocks `pnpm install` (and every command that triggers it) until you explicitly approve `esbuild`'s postinstall script. The repo's `frontend/pnpm-workspace.yaml` pre-approves it (`allowBuilds: { esbuild: true }`), so a fresh clone Just Works — if you see the error on a stale worktree, re-pull `main` or add the same key locally.

**Save it as a script** if you spin up dev shells often — drop the block into `run-dev.ps1` at the repo root and run `.\run-dev.ps1` in a fresh PowerShell.

</details>

### First prompt
1. Open the **Workspace** tab and pick a model from the dropdown.
2. Type a prompt (`Why is the sky blue?` is a good smoke test).
3. Click **Run**.
4. Watch tokens stream in. Note the metrics line below the output.

### First install from the UI
1. Open the **Models** tab (or the **Downloads** tab to add a new model).
2. Pick a source:
   - **Ollama Library** — type any model name (e.g. `mistral:7b`) and click Install.
   - **Hugging Face** — search a GGUF repo, click a result, pick a variant.
   - **Local File** — drag a `.gguf` onto the modal or click Browse.
3. Confirm any disk-space warnings, click Install, watch the progress bar.

### First transcription (voice)
1. Open **Models → Speech-to-Text**. If whisper.cpp isn't found, run `brew install whisper-cpp` (the tab has a copy button) and click **Re-check**.
2. Download a model from the catalog — **Base (English)** is a good first pick (~148 MB). The shared silero VAD comes with it automatically.
3. In the header **Speech-to-Text** control, pick the model and press **▶** to start the engine. The Workspace switches to the two-pane transcribe view.
4. Press **Record** (or upload a WAV), speak, then stop. The transcript streams into the left pane.
5. Optionally type an assistant prompt, pick an LLM in the header, and click **Ask the assistant** — or flip on **Auto-summarize** to have it run automatically.

---

## Your local model passed the demo. Will it survive an agent?

Parameter count is a terrible predictor of whether a local model can actually drive an agent. In Docker's open agent-loop benchmark (3,570 runs across 21 models), **Llama 3.3 70B scored 0.61** on tool-calling while **Qwen3 8B scored 0.97** — a model ~20× smaller was far more reliable at the thing that makes an agent useful.

The same model that's flawless in chat will, mid-task, fake a tool call, loop until it gives up, or declare "done" without finishing. And on local hardware the result also shifts with **quantization and VRAM** — so a verdict from a cloud eval doesn't transfer to your machine.

QuantaMind is the gate you run before that happens.

> [!IMPORTANT]
> Everything runs on your machine. There is no QuantaMind cloud, no account, no telemetry. Your prompts and your model outputs never leave your hardware.

---

## What it does

Point it at a model running locally. It drives that model through real agentic tasks — multi-step plans, tool calls, recovery — and returns a verdict for **that model on that hardware**:

- **Ready** — reliable enough to wire into an agent here.
- **Conditional** — works for some task tiers, not others. The report tells you which.
- **NotReady** — fails the tasks that matter, with the failure mode named.

Reliability is scored with **pass^k** — does the model succeed *k* times out of *k*, not once by luck. For an agent, consistency is the whole game.

Everything runs on-device. HTTP-only to your local backend. No weights bundled, no telemetry, no account required.

---

## How it works

- **Backends:** Ollama, llama.cpp, and MLX over HTTP — no weights bundled.
- **pass^k scoring:** *k*-of-*k* success per task tier (Easy / Medium / Hard), not a single pass.
- **Hardware-aware:** the verdict accounts for your machine — quantization, VRAM headroom, spawn-time footprint, prefix-cache behaviour, and the context window (sized to what your RAM actually holds; only the *window* scales with hardware, never the per-tier token budget, so a tier means the same thing on every machine).
- **Reasoning models, done right:** captures the model's separate reasoning stream (Ollama `thinking` / llama.cpp inline `<think>`) so it isn't discarded, and gives it a fixed, generous per-tier thinking budget via a **Lean / Standard / Deep** preset. A run that fails is diagnosed honestly — `Reasoning-overrun` (spent its whole budget thinking with memory to spare → raise the preset, a *setting*) vs `Truncated (context-bound)` (the window filled → a *hardware* limit) — so you never buy hardware to fix a budget knob.
- **Failure-mode classification:** surfaces *how* a model breaks — `ForbiddenCall` (out-of-scope tool), `LoopCap` (step budget hit), silent `FakeDone`, `Reasoning-overrun`, and more — not just a pass/fail number.
- **Quant comparison:** diff a quantized model against a Q8 baseline to catch behaviour that a raw score hides.

What a spec sheet and a cloud eval structurally cannot show: the same model at Q4 vs Q8 can hold the same score while changing *how* it fails. QuantaMind shows you that.

---

## Why local, why pre-deployment

Production observability tools tell you how your agent did *after* it shipped, on a cloud API model. QuantaMind answers a different question: **will this specific local model, on this specific hardware, hold up as an agent — before I ship it?**

A cloud SaaS can't answer that, because it doesn't run on your machine and local results are hardware-dependent. For teams forced onto local models by data-sovereignty or air-gap requirements, that gap is the whole problem.

---

## Features

### Workspace
- Monaco-based prompt editor (same editor that powers VS Code)
- Live model picker driven by `/api/tags` — what shows is what Ollama can actually serve right now
- Token-by-token streaming output in a preserved-whitespace pane
- Explicit `running` / `streaming` / `done` / `cancelled` / `error` terminal states
- Clean cancellation that cuts the HTTP stream — no fake "done" events
- Save and load prompts as YAML with byte-identical round-trip
- Per-run metrics: TTFT in ms, sustained tokens/sec, token count
- Persistent status bar: model name, Ollama health pill, latest run metrics

### Speech-to-Text (Voice)
- Local transcription via **whisper.cpp** (`whisper-server`) — its own engine axis, parallel to the LLM backend, on a fixed port `:8093`
- One-time setup walked through in **Models → Speech-to-Text**: `brew install whisper-cpp`, then download a model — QuantaMind finds the engine automatically on `PATH`/Homebrew (no path setup), with a `--help` dry-run so "found" never masquerades as "runnable"
- Curated model catalog: tiny / base / small / medium (English + multilingual), large-v3 and large-v3-turbo, plus quantized variants — real Hugging Face download sizes shown up front
- Each download is atomic — the whisper ggml **and** the shared silero VAD are promoted both-or-none; half-installs are swept at startup
- Start the engine from the header **Speech-to-Text** control (`▶` + model picker + health dot); the Workspace switches to a two-pane transcribe view while it runs
- **Record** from the mic or **upload** a WAV; audio is decoded → downmixed → resampled to 16 kHz mono **in Rust**, then sent to whisper-server one ~30 s window at a time
- **Voice → assistant**: the transcript becomes the user message, an optional typed prompt sets the system/context, and the selected LLM streams a reply — flip on **Auto-summarize** to fire the LLM automatically when a recording finishes (a production-faithful end-to-end timing)
- Offline-only by construction: a loopback-only probe means transcription never silently reaches the cloud — a down local server fails loud instead

### Model Management
- One modal, three tabs: Ollama Library, Hugging Face, Local File
- Disk-space pre-check refuses any install that would leave < 2 GB free
- Real-time progress: bytes / total / speed (5-second moving average) / ETA
- Cancel button on every in-flight install
- Storage tab with size-sorted list, family, parameter count, quantization
- One-click Uninstall guarded by an `alertdialog` confirmation
- Storage path section that shows current `OLLAMA_MODELS` and *honestly* helps you change it

### Analysis
- Top-level tab parallel to Workspace, with two sub-tabs: **Analysis** (multi-model compare) and **Quant**
- Multi-select installed models, one prompt, three strategies
- Hardware feasibility verdict: `ok` / `risky` / `wont_fit` — computed at click time
- Per-model streaming column with its own metrics row
- Throughput + TTFT comparison chart; word-level output diff
- Export full run as Markdown or JSON via save dialog

### Backends
- One `InferenceBackend` trait, three runtimes: **Ollama**, **llama.cpp** (`llama-server`), **MLX** (`mlx_lm`, Apple Silicon only)
- The backend is bound to the model's weight format — auto-picked, never a silent fallback
- Each external server is launched stream-aware (no blind timeout), reaped on app exit, and bound to a dynamically chosen free port

### Latency
- Per-token timing forensics for the last run
- TTFT breakdown: model-load vs prompt-prefill vs generation, as a stacked phase bar
- Per-token latency timeline (visx) with outlier highlighting and phase boundaries
- Inter-token latency histogram, VRAM bar, context-budget bar
- Cold- vs warm-start comparison, memory-leak heuristic, regression alerts, HTML report export

### Tests
- Score models on **graduated tiered agentic scenarios** (Easy→Extreme) — the bundled suites span coding, finance, medical, legal, ecommerce, support, supply-chain, math/science, and clinical-trial domains; users add tasks or import JSON/CSV per tier
- **Difficulty Tier** run control (`Auto`/Easy…Extreme): the chosen tier filters the Built-In list to that tier's collections and recommends both **Pass^k** (Easy 5 / Medium 8 / Hard 16 / Extreme 24) and the agentic **Max Steps** budget (Easy 8 / Medium 16 / Hard 32 / Extreme 48); `Auto` follows your machine's hardware class (shown as a "HW: …GB · class · tier recommended" hint). **Iterations (k)** and **Max Steps** are always editable — pre-filled with the tier's recommended value but yours to override. An **Anti-Saturation** toggle shuffles N never-correct decoy tools into each task to resist contamination; the scoreboard header echoes the live run as `Tier · K · Decoys`. Click a collection in the left sidebar to expand its tasks, each with hover **Edit**/**Delete** (editing a built-in saves a custom copy)
- Deterministic, sandbox-free scoring: composite tool-call accuracy (parse · tool · args · abstain), **Pass^k** reliability, avg steps, effort, **schema resilience**, dominant failure mode
- **Deterministic environments + visual replay:** beyond entity world-state, a task can run against a **simulated filesystem** the agent browses with `read_file`/`list_dir`/`grep` — getters return **real content**, not an empty ack. The Trace Debugger **replays the run visually**: a file tree beside the text trace with a step scrubber, so you *watch* the model open a config file and read its actual contents. Fully deterministic and reproducible (the picture is a pure function of the frozen environment, so it can never disagree with the score), and the replay is local-only — never published
- **Thinking-model checkbox (per model, in the model picker):** marks a model as a reasoner, so the agentic runner raises its per-turn token budget (tier-scaled 1536→4096 vs. the default 256) and strips `<think>` scratchpads before scoring — without it a reasoning model is truncated mid-thought and scored as malformed, letting a terse small model out-score a far larger reasoner for a purely structural reason. **Auto-detected** from the model name (e.g. qwen3.x, QwQ, DeepSeek-R1) so it's pre-checked and you don't have to guess; click to override. The choice persists per-model and tags the result so a thinking model's higher effort is never ranked against a terse model's
- **Dialect normalization + run-time guardrails:** a model that ignores the instructed JSON and emits its own native tool grammar (e.g. gemma's `<channel|>…call:tool{…}` format) is normalized so it can still be scored — and tagged with a **dialect badge** (e.g. "Harmony") so a model that only scored via normalization is visible, never silently credited. A **per-task wall-clock budget** caps a pathologically slow Pass^k batch (a too-large model on a too-small box, minutes per step) and reports the honest partial pass-rate over the runs that completed — flagged truncated, and never counted as a clean pass^k
- **Foreign-dialect verdict (honest, at production parity):** a mis-built model (e.g. a mis-quantized GGUF that emits unparseable harmony-ish token soup like `<|tool_response|>call:reply(text='…')`) is labeled **`foreign_dialect`** — its own failure verdict, distinct from "malformed JSON" or "hallucinated", so a template/dialect artifact is never mislabeled as a model-capability failure. Crucially these broken forms are **labeled, not salvaged**: the harness only recovers a call when a real client (Ollama's native parser) also would, so the bench can never score *more leniently than a real deployment* — the difference between "fail in the bench → succeed in production" and the inverse
- **Empty-output verdict (honest):** a model that emits nothing usable — empty / whitespace / punctuation-only (e.g. a model that doesn't engage the prompt-based JSON tool format and emits a lone `.` before its stop token) — is labeled **`empty_output`**, distinct from "hallucinated": *"the model said nothing"* is not *"the model claimed it finished"*. Such a model typically needs the **Measure native tool-calling** toggle — the same model that emits `.` on the prompt path returns clean `tool_calls` natively
- **Per-model stop tokens (no runaway generation):** harmony models (gpt-oss) end a turn on `<|return|>`/`<|call|>` and gemma on `<end_of_turn>` — none of them a plain EOS — so without the right stop sequences they emit those markers as *text* and generate until the token cap, hallucinating an entire fake multi-turn transcript. Each eval turn resolves the model's real end-of-turn tokens from its architecture (Ollama `/api/show`, metadata-only — no model reload) and passes them as `stop`, so generation halts cleanly at the tool call instead of looping
- **Context Stress Test** — backend engine that pads tasks with license-clean synthetic presets, sweeps the instruction across mid-document depths, and verifies each rung to ±5% of the target, finding the prompt length where tool-call accuracy collapses (real measured prompt tokens, never an estimate); the deep rungs are slow (a quarter-to-full context window of inference), so live progress is reported at *task* granularity — "rung r/N · padding to Nk tokens · position p/3 · task t/M" with an elapsed/ETA readout and an overall % bar — so a long rung shows continuous movement instead of looking stuck; the % bar is **monotonic and never reads a false 100%** — within-rung fill is capped below each rung boundary (it only crosses when the rung authoritatively completes) so a verify-and-adjust re-sweep can't bounce it back, and it snaps to 100% when an early-stopped probe finishes; every rung keeps a per-step trace — the exact system prompt + each needle position's output (pass or fail), streamed live per rung — surfaced as a per-row **View trace** in the results table (with an ⓘ explaining how Accuracy is scored) so a red "0% / Broken" shows *what* the model saw and emitted, not just that it failed
- Author custom collections by hand or bulk-load single-turn tasks via CSV import
- Optional native function-calling path (Ollama `/api/chat` `tools`) alongside the prompt-based proxy
- **Per-run trace inspector:** the agentic Trace Debugger splits a task's Pass^k repetitions into **collapsible "Run N of K" sections** (each with a PASS/FAIL/RUNNING chip and its own turn numbering) instead of one flat stream — so identical single-step runs no longer look like duplicate cards. The **Audit & Compliance** regression graph also refreshes live when a batch finishes for the shown collection (no app restart needed)

### Quant (Analysis → Quant sub-tab)
- Compare a model family's installed quantizations side by side
- Size · hardware fit (OOM risk) · quality (eval pass-rate) · tool-call composite
- Recommends the best size↔quality↔fit trade-off for your use case and context length

### Agent Report
- Per-model **Ready / Conditional / Not Ready** verdict with the exact blocking + conditional reasons
- Hardware-aware: VRAM fit (exact weights + KV cache vs an allocation cap, with a pressure flag)
- Configurable readiness profiles (min Pass^k, forbid loops/false-done, require full VRAM, min context, require native FC)
- **Both tool-calling paths, side by side:** a model measured on both native function-calling and the prompt-based proxy gets a **separate verdict row per path** (labelled **Native FC** / **Prompt-Based**), each gated on its own Pass^k — so a model that passes the prompt proxy but fails native reads Not Ready on the path your app actually uses. Native availability is judged at the model level (a native-capable model's prompt-based row is never falsely failed by a "require native FC" profile); the **Show Native-FC Path** toggle hides the native rows
- **Per-model deep-dive (Phase 9B):** an **Executive Verdict** judged against the tier you actually ran (hardware class shown as advice, never a forced fail), a **Tier Progression Matrix** (per-tier Pass^k + avg-steps with CLEAR / SATURATED / FAIL / NOT-TESTED badges — saturation at a glance), and a **Failure Taxonomy** (decoy / forbidden-call / loop / hallucination distribution across the tested tiers) — the selector targets a specific *(model, path)*. The matrix **accumulates per domain**: running Easy then Medium for the same domain fills in both tiers (the assessment unions a model's ladder across the domain's tier collections, matched per `(model, backend)` and same path), and the report **auto-refreshes** when a batch finishes for the shown collection or a same-domain tier — no manual re-run to see a newly-tested model/tier
- Export the verdict table as a standalone HTML report, or the deep-dive as versioned JSON
- **Publish to the community leaderboard (opt-in, default-OFF):** an allowlisted, **verdicts-and-metrics-only** payload — model, quant, hardware `cohort_key`, the tier verdict (status / tier-tested / cleared / recommended), the per-tier Pass^k curve, the failure-mode distribution (counts), the collection identity + content hash, and build provenance (`schema_version` / `engine_version` / a `build.rs` git hash). Never prompts, traces, machine identifiers, or results on your own custom collections; the dialog shows the exact JSON before anything leaves the machine. A row is publishable once it has a measured Pass^k and a known quantization — the quant is read from the Ollama registry or, failing that, parsed from the model name, so **llama.cpp / MLX (and offline-Ollama) results publish too** instead of erroring with "no rows". A model measured on both tool-calling paths publishes **one row per path** (distinguished by `eval_method`), so the leaderboard can compare native vs prompt-based directly

---

## Roadmap

- **WebGPU** — run the readiness gate in the browser, on your own GPU, zero install.
- **Windows + Linux** desktop builds — *in progress*. Platform-adapter foundation (Unix + Windows `EngineHost`) has landed; sidecar lifecycles (Ollama auto-start, `llama-server`, `whisper-server`), multi-vendor GPU probe (NVIDIA + AMD + Intel + DXGI), and native Windows storage paths are being rewired phase-by-phase.
- **Expanded task suite** — more agentic tiers and domains.
- **Deterministic visual environments** — stateful WebUI + vision/OCR readiness evals.

---

## Tech stack

> [!NOTE]
> These choices are **locked**. Substitutions require explicit review.

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | **Tauri 2.x** | ~30 MB binaries, native WebView, Rust backend |
| Backend language | **Rust 1.75+** (ed. 2021) | Tauri default; safe IPC + HTTP |
| Frontend framework | **React 18 + TypeScript 5** | Largest open-source contributor pool |
| Build tool | **Vite 5** | Fast HMR, Tauri-friendly |
| Styling | **Tailwind CSS 3** | Utility-first, no design-system overhead |
| State management | **Zustand** | ~1 KB, no boilerplate, scales |
| Editor | **`@monaco-editor/react`** | Same editor as VS Code |
| HTTP client (Rust) | **`reqwest` + `tokio`** | Battle-tested |
| Speech-to-text engine | **whisper.cpp** (`whisper-server` sidecar) | Local STT over HTTP on `:8093`, mirroring the `llama-server` lifecycle; subprocess, not FFI |
| Audio preprocessing (Rust) | **`hound` + `rubato`** | Decode WAV → downmix → resample to 16 kHz mono in-process, explicit and logged |
| Voice-activity detection | **`webrtc-vad`** | Independent, non-ML VAD for the silence-hallucination metric (never the STT model's own opinion) |
| Serialization | **`serde` + `serde_json` / `serde_yaml`** | Type-safe across IPC |
| Validation (TS) | **`zod`** | Runtime schema validation at IPC boundary |
| Validation (Rust) | **`validator` + `serde`** | Type-level + custom validators |
| Testing (Rust) | **`cargo test` + `mockito`** | Built-in, no setup |
| Testing (TS) | **`vitest` + `@testing-library/react`** | Fast, Vite-native |
| Format / Lint | **`rustfmt` + Prettier / Clippy + ESLint** | Auto-format on save |
| Pre-commit | **`lefthook`** | Lighter than Husky |
| CI | **GitHub Actions** | Free for open source |

<details>
<summary><b>What is deliberately NOT installed (yet)</b></summary>

- No logging library — `println!` / `console.log`; structured persistence is deliberately deferred
- No state-machine library — Zustand is enough
- No UI component library — Tailwind utilities only
- No form library — there are no real forms yet
- No in-process AI/ML libraries — QuantaMind *calls* Ollama; it doesn't run inference itself

Every dependency is a maintenance debt. Resist additions.

</details>

---

## Architecture

```
┌────────────────────────────────────────────────────────────┐
│                  QuantaMind Desktop App                    │
│                                                            │
│  ┌────────────────────────────────────────────────────┐    │
│  │            React + TypeScript Frontend             │    │
│  │   features/  ←  shared/ipc/  ←  Tauri invoke()     │    │
│  └──────────────────────────┬─────────────────────────┘    │
│                             │                              │
│                    IPC boundary (JSON)                     │
│                             │                              │
│  ┌──────────────────────────▼─────────────────────────┐    │
│  │               Rust Backend (backend/)              │    │
│  │   commands/  →  inference/  →  metrics/            │    │
│  │        ↓                                           │    │
│  │   persistence/                                     │    │
│  └──────────────────────────┬─────────────────────────┘    │
└─────────────────────────────┼──────────────────────────────┘
                              │ HTTP
                              ▼
                ┌─────────────────────────────┐
                │   Ollama (localhost:11434)  │
                └─────────────────────────────┘
```

**Module boundaries**

| Side | Module | Responsibility |
|---|---|---|
| Frontend | `app/` | App shell, routing, providers. No feature logic. |
| Frontend | `features/<name>/` | Vertical slice: components, hooks, state, types, tests. Deletable in one `rm -rf`. |
| Frontend | `shared/ipc/` | **Only** place that calls Tauri `invoke`. Typed wrappers + zod schemas. |
| Frontend | `shared/components/` | Primitives reused by 2+ features. |
| Backend | `commands/` | IPC entry points. Thin: validate, call domain, return. |
| Backend | `inference/` | Backend adapters behind the `InferenceBackend` trait — Ollama, llama.cpp, MLX — plus the eval/readiness scoring engines. |
| Backend | `metrics/` | TTFT, tokens/sec, per-token timeline, VRAM. |
| Backend | `persistence/` | YAML/JSON read+write for prompts and history. |
| Backend | `validation/` | Schemas shared by commands and persistence. |
| Backend | `errors.rs` | Single `AppError` enum. **No `unwrap()` outside tests.** |

The two halves talk JSON over Tauri's IPC. Contracts are explicit in `shared/ipc/types.ts` on the TS side, mirrored in Rust — no codegen.

> **New here?** [`ARCHITECTURE.md`](ARCHITECTURE.md) is the five-minute map of the
> backend's hexagonal layout and the one dependency rule that keeps it navigable.

---

## Building from source

### Development build
```bash
cd frontend
pnpm install
pnpm tauri dev
```

### Production build
```bash
cd frontend
pnpm install
pnpm tauri build
```

Outputs land in `backend/target/release/bundle/`:
- macOS: `.dmg` and `.app`

> **macOS is the shipping target today.** Windows and Linux support is being rewired phase-by-phase — the platform-adapter foundation has landed but the runtime lifecycles aren't fully cross-platform yet. See the [Roadmap](#roadmap).

### Run the test suites

```bash
# Frontend tests (vitest)
cd frontend
pnpm test

# Backend tests (cargo test + mockito)
cd backend
cargo test
```

---

## Project layout

```
QuantaMind/
├── .github/
│   └── workflows/{ci.yml,release.yml,nightly.yml}
│
├── frontend/                       # React + TS + Vite
│   ├── src/
│   │   ├── app/                    # Shell, routing, providers
│   │   ├── features/
│   │   │   ├── workspace/          # Prompt + run + stream + YAML I/O
│   │   │   ├── models/             # Install / browse / storage
│   │   │   └── compare/            # Multi-model side-by-side
│   │   ├── shared/
│   │   │   ├── components/
│   │   │   ├── ipc/                # ONLY place that calls invoke()
│   │   │   ├── format/
│   │   │   └── styles/tokens.css
│   │   ├── main.tsx
│   │   └── index.css
│   ├── index.html
│   ├── package.json
│   ├── vite.config.ts
│   ├── vitest.config.ts
│   └── tailwind.config.js
│
├── backend/                        # Rust + Tauri 2
│   ├── src/
│   │   ├── main.rs / lib.rs
│   │   ├── commands/               # IPC entry points
│   │   ├── inference/              # Ollama, HF, GGUF, chat-templates
│   │   ├── metrics/                # TTFT, tokens/sec
│   │   ├── persistence/            # YAML/JSON
│   │   ├── validation/             # Shared schemas
│   │   ├── sync.rs                 # Mutex poison recovery
│   │   └── errors.rs               # AppError enum
│   ├── tests/                      # Integration tests (cargo convention)
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── docs/                           # Architecture + workflow docs
│   └── codebase/                   # File-by-file reference (Why/What/How)
├── CLAUDE.md                       # Project instructions
├── README.md                       # ← you are here
└── LICENSE
```

> [!NOTE]
> **Every source file is single-concern.** Split a file when it starts doing *two things*, not when it crosses a line count. See [Engineering principles](#engineering-principles).

> [!TIP]
> **Want the full map?** [`docs/codebase/`](./docs/codebase/README.md) is a deep,
> file-by-file reference for the entire codebase — every backend module and
> frontend page/tab/feature, with **Why** it exists, **What** it does, and
> **How/Where** it's used. Start at its [index](./docs/codebase/README.md) and
> jump to the subsystem you need (e.g.
> [inference engines](./docs/codebase/backend-inference-backends.md),
> [eval](./docs/codebase/backend-eval-engine.md),
> [the Workspace tab](./docs/codebase/frontend-workspace.md)).

---

## Deep dive — Workspace

### The flow
1. Open QuantaMind. The Model Picker fetches `/api/tags` from your local Ollama.
2. Type or paste a prompt into the Monaco editor.
3. Click **Run**.
4. Tokens stream into the output pane in real time.
5. The run terminates with one of:
   - **Done** — full metrics shown (TTFT, tok/s, token count)
   - **Cancelled** — distinct amber state, token count only
   - **Error** — typed error surfaced as a banner
6. Optionally save the prompt to a YAML file for reuse.

### State separation rule
A codified rule across the codebase:

> **Hooks own ephemeral per-action state; the store owns shared cross-component state.** Hooks may write to the store at completion. Components must not read both for the same data — pick one source.

In practice: the currently-streaming output and run status live in `useStreamingRun`'s local `useState`. The most-recent run's final metrics are written into the Zustand `useWorkspaceStore` so the StatusBar (a separate part of the tree) can render them without prop-drilling.

### YAML round-trip
`StoredPrompt { model, prompt }` serializes with `serde_yaml` to a two-field document. Loading produces the same struct; saving it back produces a **byte-identical** file. The round-trip is asserted in the test suite for ASCII, multi-line (block scalar form `|-`), and UTF-8-with-embedded-quotes payloads.

### Cancellation
A `tokio_util::sync::CancellationToken` threads through the entire HTTP stream:
- An outer `tokio::select!` races the token against the next byte chunk
- An inner per-token check breaks the line-parse loop early
- The backend emits a dedicated `prompt-cancelled` event (**not** `prompt-done`) so the UI can render a distinct terminal state

### Timeouts
Every cross-process call has a budget:

| Call | Timeout |
|---|---|
| `run_prompt` | 30 s outer + 60 s reqwest connect; no per-request timeout on the stream |
| `stop_prompt` | 5 s |
| `list_models` | 5 s |

Timeouts surface as `AppError::Timeout` with a human-readable label.

---

## Deep dive — Speech-to-Text

Speech-to-text is an **additive, parallel capability**. It never touches the `InferenceBackend` trait or `run_prompt` — the STT engine is its own state axis, so one whisper.cpp engine runs alongside one LLM.

### Setting up the engine
The **Models → Speech-to-Text** tab drives a three-state flow off a single engine check:

| Engine state | What you see |
|---|---|
| Not installed | A setup card: install [Homebrew](https://brew.sh), run `brew install whisper-cpp`, click **Re-check** |
| Installed but not runnable | A reinstall card (`brew reinstall whisper-cpp`) — the binary is present but its dylibs are missing/mismatched |
| Ready | The model catalog + server controls |

The binary is discovered **most-explicit-first**: `UserSettings.stt_engine_dir` → `QUANTAMIND_WHISPER_DIR` → `PATH`/Homebrew → bundled resources → dev tree. So a plain `brew install whisper-cpp` is found with no path setup; installed it elsewhere? **Choose its folder** (remembered across launches). QuantaMind only reports ready after a `--help` dry-run proves the engine actually executes — "found" never masquerades as "runnable".

### Models
All whisper ggml models come from `ggerganov/whisper.cpp`; the shared silero VAD (`ggml-silero-v6.2.0.bin`) comes from `ggml-org/whisper-vad`. The curated catalog (real HF sizes shown before download):

| Model | Size | Languages |
|---|---|---|
| Tiny / Base / Small / Medium (`.en`) | 78 MB – 1.5 GB | English |
| Tiny / Base / Small / Medium | 78 MB – 1.5 GB | Multilingual |
| Large v3 / Large v3 Turbo | 3.1 GB / 1.6 GB | Multilingual |
| Quantized variants (`q5_0` / `q5_1`) | 32 MB – 1.1 GB | much smaller, near-identical accuracy |

`download_stt_model` stages the whisper ggml **and** the VAD, validates both, and promotes **both-or-none**; `reconcile_stt_dir` sweeps half-installs at startup. Runtime VRAM isn't measured yet, so the UI shows **"Not available"** rather than a fabricated figure.

### The transcription seam
```
record / upload WAV
  ↓  hound (decode) → downmix → rubato (resample → 16 kHz mono), in Rust, logged
whisper-server /inference   (one call per ~30 s window, verbose_json)
  ↓  stream segments through TranscribeSink
canonical Transcript        (persisted atomically; an incomplete run is refused)
```
Every `TranscribeStats` field is `Option` — no fabricated metric.

### Voice → assistant
The transcript becomes the **user message**; an optional typed prompt is the **system/context** (e.g. *"You are a customer support agent"*). Both go to the selected LLM, which streams its reply through the same rich metrics path as a normal Workspace run. With **Auto-summarize** on, the LLM fires automatically the moment a transcription completes (the STT→LLM auto-pipe), so the end-to-end time is production-faithful.

### Server lifecycle & failure states
The sidecar runs on a fixed `:8093` (clear of MLX's `8082..=8092` scan range) with **`/health`-gated readiness** — HTTP 200 once the model is loaded, 503 while loading — graceful-then-hard kill, and reaping on app exit alongside the LLM sidecars. Start failures (`start_whisper_server`) return a tagged result so the UI says exactly what to fix:

| Tag | Meaning |
|---|---|
| `not_bundled` | No `whisper-server` found — install whisper.cpp |
| `model_missing` | The whisper model file isn't on disk — download one first |
| `vad_missing` | The silero VAD is absent — re-run the download |
| `port_conflict` | Something else holds `:8093`; QuantaMind won't take over a process it didn't start |

A server that won't answer on `127.0.0.1` fails **loud** — STT is offline-only and never falls back to the cloud.

---

## Deep dive — Model Management

The largest piece of code in the app, because installing models cleanly is *hard*.

### Three sources

<details open>
<summary><b>Ollama Library</b> — free-text input + Install</summary>

Type any model name Ollama knows (`mistral:7b`, `qwen2.5:14b`, …) and the backend's `pull_model` command POSTs `/api/pull` with `stream: true`, parses NDJSON line-buffered, and emits per-chunk progress events.

A live `list_models` subscription marks the typed name **Installed ✓** the moment it appears.

</details>

<details>
<summary><b>Hugging Face</b> — search the live HF API</summary>

```
GET https://huggingface.co/api/models
  ?search=<q>&library=gguf&sort=downloads&direction=-1&limit=30
```

Clicking a result opens a detail view that lists every `.gguf` file in the repo via `GET /api/models/{repo}/tree/main?recursive=true`. The quantization label is parsed from the filename (`Q4_K_M`, `IQ4_XS`, `BF16`, …); the install name is derived as `<basename>:<quant>` to satisfy Ollama's name validator.

Downloads stream to a `.partial` file with HTTP `Range` header support, so an install **resumes** on the next click after the app is closed mid-download. A *failed* or cancelled download is cleaned up automatically — the incomplete `.partial` (and any half-written file) is deleted so nothing broken is left behind and a retry starts fresh. After download, the GGUF header is parsed in Rust to read architecture/quant/context; the parser grows its metadata read window on demand (8 MiB → up to 256 MiB) so large-vocab tokenizers (e.g. Qwen3) parse cleanly instead of falsely reporting a truncated file.

</details>

<details>
<summary><b>Local file</b> — drag-and-drop or Browse</summary>

QuantaMind reads the first 64 KB of the `.gguf` file with a pure-Rust GGUF v3 header parser to extract:

- architecture (`llama`, `qwen2`, `phi3`, …)
- parameter count
- context length
- quantization (from `general.file_type`, with filename fallback)
- family (derived from architecture)

The parser handles the full GGUF value-type set (`UINT8`…`FLOAT64`, including `UINT16`/`INT16`/`FLOAT64`), so models that store scalar metadata in narrow integer types — e.g. some Qwen 2.5 Coder exports — parse instead of failing on an unsupported value tag.

The user reviews metadata, picks a name (validated by regex + against existing models), clicks Import.

</details>

### What "Import" does

For both HF and Local sources, the keystone pipeline:

```
inspect_gguf(path)              → GgufMetadata
  ↓
detect_template(name, arch)     → Option<ChatTemplate>
  ↓
generate_modelfile(spec)        → Modelfile string
  ↓
ollama_create(name, modelfile)  → POST /api/create (NDJSON stream)
  ↓
verify_model_registered(name)   → backoff poll of /api/tags
  ↓
emit("models-changed")          → refresh installed list everywhere
```

The **chat template registry** covers 8 families today: Llama 3, Qwen/ChatML, Mistral, Phi-3, Gemma, Command-R, DeepSeek, Yi. Architecture is checked first (more reliable than name); name substring is a fallback. Unknown families return `None` so the UI surfaces a warning rather than emitting a wrong-template Modelfile.

### Disk pre-check

Before any install path downloads:

```
free = current free disk space at $OLLAMA_MODELS
need = estimated_size_bytes × 1.05   # 5 % safety margin

if free < 2 GB  after install → BlockedInsufficientSpace
if free < 10 GB after install → Warning
else                          → Ok
```

Constants are justified in code: 2 GB covers OS swap and app caches; 10 GB covers a week of the user's other work.

### Storage view

Sorted by size descending. Each row: family · parameter size · quantization · size. Uninstall opens an `alertdialog` that names the model and the bytes it frees. Only the Remove button (red) calls `DELETE /api/delete`. On success the backend emits `models-changed` and every consumer in the app re-fetches once.

---

## Deep dive — Analysis

### Strategy choice

All three strategies are always visible so users can compare verdicts before picking. The Run button re-validates at click time and refuses to run a `wont_fit` strategy.

| Strategy | Memory needed | Wall-clock today | Why it exists |
|---|---|---|---|
| **Sequential** | `max(model_size)` | `N × per-model latency` | Safest default |
| **Parallel** | `sum(model_size)` | ≈ sequential (Ollama serializes) | Honest about issue time; ready for a second backend |
| **Sequential w/ skip** | `max(model_size)` | ≤ sequential | Bail out when you've seen enough |

### Feasibility math

```
required(model)        = ceil(size_bytes × 1.3)   # runtime > on-disk
sequential / skippable = max(required)
parallel               = sum(required)

verdict = need > avail        ? "wont_fit"
        : need > avail × 0.7  ? "risky"
                              : "ok"
```

`avail` comes from `sysinfo::System` available memory. On Apple Silicon it's labelled "Unified memory"; no discrete-VRAM probe yet.

### Event bus

Backend emits five `compare-*` events, each payload carries a per-row `model_id` (UUID generated at invoke time):

| Event | Payload |
|---|---|
| `compare-token` | `{ model_id, model, text }` per chunk |
| `compare-done` | `{ model_id, model, ttft_ms, tokens_per_sec, token_count }` |
| `compare-cancelled` | `{ model_id, model, token_count }` |
| `compare-error` | `{ model_id, model, kind, message }` |
| `compare-run-done` | Terminator; flips pending rows to `cancelled` |

### Export

`buildReport(...)` snapshots the store at click time. `toMarkdown(report)` and `toJson(report)` are pure functions of that value — streaming after Export doesn't mutate what was saved. The user picks a destination via `plugin-dialog::save`; the backend writes the file.

---

## Install pipeline internals

The install path has a shared invariant:

> [!IMPORTANT]
> **A successful install must be observable in the UI before the install hook returns "success".**

Getting there required hardening against several known Ollama 0.24+ behaviors.

### The `/api/tags` lag race
Ollama streams `{"status":"success"}` from `/api/create` *before* the new model is reflected in `/api/tags`. Observed lag: **50–800 ms**. A naive one-shot check races and reports a false "silently rolled back" error.

`verify_model_registered` solves this with a backoff ladder (50 / 100 / 200 / 400 / 800 / 1500 ms) plus a final confirmation read. Only after **all seven checks miss** does it declare a rollback.

### The un-terminated final-line problem
Ollama 0.24+ has been observed to close the HTTP connection with the final `{"status":"success"}` un-flushed and *un-terminated by a newline*. A line-based NDJSON parser drops that chunk and concludes the stream failed.

`backend/src/inference/ndjson.rs` flushes the un-terminated remainder via `ndjson::tail(&buf)` after stream close and re-runs the same chunk parser. Five integration tests cover the with-newline and without-newline success paths.

### Terminal states stay visible
The "In progress" list previously filtered to `["downloading", "installing"]` only, so entries vanished the instant status flipped to `success` or `error`. The list now retains terminal entries with badges and a Dismiss button. Success entries auto-clear after 5 s; error entries persist until dismissed.

### Self-healing refresh
`installedModelsStore` is the single source of truth for the installed list. A `models-changed` event bus mirrors `downloadEventBus`: one shared `listen("models-changed")` subscription mounted once in `App.tsx`, dispatching into the store. All five consumers subscribe to the store and fall back to calling `refresh()` themselves when status is `idle` — a self-heal that survives Tauri listener-registration races.

`remove_model` emits the same `models-changed` event, symmetric with install paths.

---

## Live model browsing

QuantaMind originally shipped bundled JSON catalogs of "popular" models. Those went stale fast. The current build replaces them with **live API queries**.

| Surface | Source | Fallback |
|---|---|---|
| Hugging Face search | `GET /api/models?library=gguf&...` | Inline error + Retry button |
| HF repo detail | `GET /api/models/{repo}/tree/main?recursive=true` | Inline error + Retry button |
| Ollama Library | Free-text input + live `list_models` subscription | Ollama's own error (404 → "not found") |

All backend HTTP clients set `User-Agent: quantamind/<version>`. HF behind Cloudflare can 400 on an empty UA; this prevents that.

---

## Engineering principles

> [!WARNING]
> These rules are non-negotiable. They're enforced in `CLAUDE.md` and applied to every change before it lands.

1. **One step at a time.** Don't start step N+1 until step N is implemented, its test passes, *and* its output is verified.
2. **Test pass ≠ data quality pass.** A green test proves the code ran the path you asked it to. The *output* must also match expected shape and values.
3. **Single-concern files.** Split a file when it starts doing two things — by responsibility, not by line count. (Folder taxonomy: ≤ 10 files per folder.)
4. **Separation of concerns.** Each file does one thing. No `utils.ts`, `helpers/`, `common/`, or `misc/`.
5. **Documentation ships with the change.** Same commit.
6. **Locked tech stack.** Substitutions require explicit review.

### The data-quality gate

After every green test, run through this checklist:

| Check | Looking for |
|---|---|
| **Shape** | Correct types, required fields present, no surprise fields; for streams: chunk count, ordering, terminator |
| **Values** | Within reasonable ranges; correct units (ms vs s, bytes vs MB); correct encoding (UTF-8, no BOM) |
| **Edge cases** | Empty input → empty output (not crash); large input handled or rejected; Unicode/emoji/RTL preserved; malformed input → typed error, not panic |
| **Cross-boundary fidelity** | Rust → JSON → TS field round-trip (snake_case vs camelCase!); disk → memory YAML byte-identical |
| **Determinism** | Same input → same output where applicable |

If verification fails, **fix the code, not the assertion**.

---

## Development workflow

The mandatory loop:

```
[1] Understand the step
[2] Implement the minimum
[3] Write the test
[4] Run the test
[5] Verify the output (data-quality gate)
[6] Update docs
[7] Commit (Conventional Commits)
[8] Move on
```

### Common violations (don't do)
- **Stacking steps** — "Let me knock out 1–3 then test." No.
- **Loosening assertions** — If `assert eq 42` fails because output is 41, fix the code.
- **Skipping verification because the test passed** — tests verify the path you wrote; verification confirms the path was right.
- **Bundling docs into "I'll update them later"** — later does not exist.
- **Refactoring during a feature** — open a separate branch.

### Naming conventions

| Domain | Style | Example |
|---|---|---|
| Rust functions / vars | `snake_case` | `run_prompt` |
| Rust types | `PascalCase` | `InferenceBackend` |
| Rust constants | `SCREAMING_SNAKE` | `DEFAULT_TIMEOUT_MS` |
| TS functions / vars | `camelCase` | `runPrompt` |
| TS components / types | `PascalCase` | `PromptEditor` |
| React component file | `PascalCase.tsx` | `PromptEditor.tsx` |
| TS non-component file | `kebab-case.ts` | `use-streaming-run.ts` |
| Rust file | `snake_case.rs` | `ollama.rs` |
| Git branch | `<type>/<short-description>` | `feature/streaming-output`, `fix/native-budget` |

### Commits — Conventional Commits

| Prefix | Use for |
|---|---|
| `feat:` | New user-visible behavior |
| `fix:` | Bug fix |
| `chore:` | Tooling, deps, config |
| `docs:` | Documentation only |
| `test:` | Adding or fixing tests |
| `refactor:` | No behavior change |

One step = one commit (or a tight related series). PR title matches the convention. PR body references `closes #N` when applicable.

### Errors
- **Rust:** `Result<T, AppError>` only. **No `unwrap()` outside tests.** Enforced by Clippy `deny(unwrap_used)` in critical files.
- **TypeScript:** discriminated unions returned across IPC. No thrown errors over the IPC boundary.

---

## Testing

### Frontend (vitest + Testing Library)
```bash
cd frontend
pnpm test            # run once
pnpm test --watch    # watch mode
```

Tests live next to code in `__tests__/` directories. Naming: tests are named after the behavior, not the function — `streams_tokens_in_order` not `test_run_prompt`. One behavior per test.

### Backend (cargo + mockito)
```bash
cd backend
cargo test           # unit + integration
cargo clippy --tests # lint
cargo fmt --check    # format
```

Inline `#[cfg(test)]` for unit tests; `backend/tests/` for integration (cargo convention). Mockito serves fixture responses for HTTP-dependent code paths.

### What we test
- **Stream ordering** — tokens arrive in correct order, byte-exact concat matches fixture
- **Cancellation** — no orphan tokens after cancel; HTTP connection closes
- **YAML round-trip** — save → load → save produces byte-identical files
- **IPC validation** — malformed payloads rejected with typed errors, no `NaN`/`undefined` reaching UI
- **Mutex poison recovery** — panic mid-lock doesn't crash; metrics degrade gracefully
- **Timeout enforcement** — `run_prompt` rejects with `AppError::Timeout` after 30 s
- **GGUF parsing** — architecture, param count, quant extracted correctly from real header bytes
- **Chat template detection** — 20+ real-world model names map to correct families

---


## Contributing

Contributions welcome. Before you open a PR:

1. **Read [`CLAUDE.md`](./CLAUDE.md)** — the engineering principles are non-negotiable.
2. **Read [`docs/process.md#workflow`](./docs/process.md#workflow)** — the one-step-at-a-time loop.
3. **Read [`docs/process.md#conventions`](./docs/process.md#conventions)** — naming, commits, branches.
4. **Keep each file single-concern.** Split by responsibility when it starts doing two things — not by a line count.
5. **Tests pass AND outputs are verified.** A green CI run is necessary, not sufficient.

### Branch naming
`<type>/<short-description>` (kebab-case), where `<type>` names the change:
`feature/` (new behavior), `fix/` (correctness fix), `bug/` (a reported bug), or
`docs/` · `chore/` · `refactor/` for non-code changes. E.g. `feature/persistent-settings`,
`fix/streaming-cancel`, `bug/empty-output-verdict`.

### Pull request checklist
- [ ] Single concern (one feature, one bug, one refactor)
- [ ] Tests added/updated and passing
- [ ] Output verified — actually look at what the code produced
- [ ] Docs in `docs/` updated in the same PR
- [ ] Each file stays single-concern (split by responsibility, not line count)
- [ ] No `unwrap()` outside tests
- [ ] Commit messages follow Conventional Commits

---

## Security & privacy

> [!IMPORTANT]
> QuantaMind is local-first by design.

| Guarantee | How it's enforced |
|---|---|
| **No telemetry** | No analytics SDK; no crash reporting service; no tracking pixels |
| **No account required** | App runs offline once a model is installed |
| **Network calls limited to** | Local model servers (`http://localhost:11434` Ollama, `127.0.0.1:8093` whisper.cpp STT, the dynamic llama/MLX ports) and `https://huggingface.co` (only when you actively browse/install/download) |
| **Speech-to-text is offline-only** | Transcription uses a loopback-only probe — it never reaches the cloud; a down local STT server fails loud rather than silently falling back |
| **No silent shell edits** | Changing `OLLAMA_MODELS` *generates* the export command for you; never edits your shell profile |
| **Tauri sandboxing** | Capabilities declared in `backend/capabilities/`; webview can only call IPC commands explicitly registered |
| **Schema validation at every IPC boundary** | Zod on TS side, serde + `validator` on Rust side; malformed payloads rejected with typed errors |
| **No `unwrap()` in production paths** | Clippy `deny(unwrap_used)` enforced in critical files; mutex-poison paths recover instead of panicking |

### Reporting vulnerabilities
Please open a [private security advisory](https://github.com/QuantaMinds/QuantaMind/security/advisories/new) instead of filing a public issue.

---

## FAQ

<details>
<summary><b>Is QuantaMind a chat app?</b></summary>

No. QuantaMind is a workbench. Each Workspace run is a single prompt → single completion (run history is available via the History panel). The multi-step **agentic** loops live in the Eval engine, not a chat UI — if you want a chat front-end on top of Ollama, look elsewhere.

</details>

<details>
<summary><b>Does QuantaMind fine-tune or train models?</b></summary>

No. QuantaMind consumes pre-trained models. Training is out of scope.

</details>

<details>
<summary><b>How does speech-to-text work, and does my audio leave the machine?</b></summary>

Speech-to-text runs entirely locally on **whisper.cpp** (`whisper-server`), a sidecar on `127.0.0.1:8093` — its own engine axis, parallel to the LLM backend. Install it once with `brew install whisper-cpp` and download a model under **Models → Speech-to-Text**. Your audio is decoded and resampled in Rust and sent only to the local server; a loopback-only probe means it never reaches the cloud, and a down server fails loud instead of silently falling back. You can pipe the transcript straight into the selected LLM (optionally automatically) for a full voice → assistant loop.

</details>

<details>
<summary><b>Why Ollama and not llama.cpp directly?</b></summary>

Ollama gives us a clean HTTP API, a stable model storage convention, and handles a lot of platform-specific GPU plumbing. It's no longer the only backend, though: a llama.cpp (`llama-server`) adapter and an MLX (`mlx_lm`, Apple Silicon) adapter now ship alongside it, all behind a single `InferenceBackend` trait.

</details>

<details>
<summary><b>Why keep files single-concern?</b></summary>

Long files hide their dependencies, smuggle in second concerns, and make every reviewer scroll. We split by responsibility — when a file starts doing two things — rather than enforce an arbitrary line count.

</details>

<details>
<summary><b>Can I run QuantaMind without an internet connection?</b></summary>

Yes, once you've installed at least one model. The Workspace, Voice (Speech-to-Text), and Analysis tabs are fully offline. Only the Hugging Face tab — and downloading new LLM or whisper models — needs connectivity.

</details>

<details>
<summary><b>What happens to my prompts?</b></summary>

They live in memory until you save them. Save creates a YAML file at the path you choose. No cloud sync. No backup. You own them.

</details>

<details>
<summary><b>How do I uninstall a model?</b></summary>

Open the Add Model modal → Storage tab → Uninstall on the row → confirm. The model is removed from Ollama and every list in the app refreshes.

</details>

<details>
<summary><b>Does QuantaMind send any usage data?</b></summary>

None. The only outbound HTTP is to your local Ollama and (when you ask) to Hugging Face.

</details>

---

## License

Apache 2.0 — see [`LICENSE`](./LICENSE).

---

## Acknowledgements

QuantaMind stands on the shoulders of:

- **[Tauri](https://tauri.app/)** — the desktop shell
- **[Ollama](https://ollama.com/)** — the local model runtime
- **[Hugging Face](https://huggingface.co/)** — the GGUF ecosystem
- **[llama.cpp](https://github.com/ggerganov/llama.cpp)** — the GGUF format and inference primitives
- **[whisper.cpp](https://github.com/ggerganov/whisper.cpp)** — the local speech-to-text engine and ggml models
- **[Monaco Editor](https://microsoft.github.io/monaco-editor/)** — the prompt editor
- **[Zustand](https://github.com/pmndrs/zustand)**, **[Zod](https://zod.dev/)**, **[Vite](https://vitejs.dev/)**, **[React](https://react.dev/)**, **[Tailwind CSS](https://tailwindcss.com/)** — the frontend stack
- **[reqwest](https://github.com/seanmonstar/reqwest)**, **[tokio](https://tokio.rs/)**, **[serde](https://serde.rs/)** — the backend stack
- The open-weights model communities — Meta, Mistral, Qwen, Microsoft, Google, DeepSeek, and many others

---

<div align="center">

**Built with discipline. Local-first by design.**

<sub>Made by QuantaMind</sub>

</div>

<div align="center">

# QuantaMind

**The pre-deployment gate for local AI agents.**

Benchmark any **Ollama**, **llama.cpp**, or **MLX** model for *agentic readiness* on your own hardware — and get a **Ready / Conditional / Not Ready** verdict before you wire it into an agent. Nothing leaves the machine.

<sub>Local-first · No telemetry · No account · pass^k scoring · hardware-aware · one ~30 MB binary</sub>

<br/>

![Version](https://img.shields.io/badge/version-0.2.0-blue)
![Platform](https://img.shields.io/badge/platform-macOS-blue)
![Tauri](https://img.shields.io/badge/Tauri-2.x-FFC131?logo=tauri&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

[![Repo](https://img.shields.io/badge/GitHub-QuantaMinds%2FQuantaMind-181717?logo=github)](https://github.com/QuantaMinds/QuantaMind)
[![Website](https://img.shields.io/badge/Website-quantamind.co-2563EB?logo=googlechrome&logoColor=white)](https://quantamind.co/)

<br/>

![QuantaMind — the readiness gate for local AI agents: qualify the exact model, quantization, runtime, and hardware you will deploy, and get one answer: Ready, Conditional, or Not Ready. Fully offline.](docs/screenshots/hero.png)

</div>

---

## Why

Parameter count is a terrible predictor of whether a local model can drive an agent. In Docker's open agent-loop benchmark, **Llama 3.3 70B scored 0.61** on tool-calling while **Qwen3 8B scored 0.97** — a model ~20× smaller was far more reliable.

The same model that's flawless in chat will, mid-task, fake a tool call, loop until it gives up, or declare "done" without finishing. On local hardware the result also shifts with **quantization and VRAM** — so a cloud eval's verdict doesn't transfer to your machine.

**QuantaMind is the gate you run before that happens.** Point it at a local model, and it drives that model through real agentic tasks — multi-step plans, tool calls, recovery — then returns a verdict for *that model on that hardware*:

| Verdict | Meaning |
|---|---|
| 🟢 **Ready** | Reliable enough to wire into an agent here. |
| 🟡 **Conditional** | Works for some task tiers, not others. The report says which. |
| 🔴 **Not Ready** | Fails the tasks that matter — with the failure mode named. |

Reliability is scored with **pass^k** — does the model succeed *k* of *k* times, not once by luck. For an agent, consistency is the whole game.

> [!IMPORTANT]
> Everything runs on your machine. There is no QuantaMind cloud, no account, no telemetry. Your prompts and model outputs never leave your hardware.

---

## Quick start

**Zero to a running window in ~5 minutes.** macOS is first-class today; Windows and Linux are landing in phases (see [Roadmap](#roadmap)). There are no prebuilt downloads yet — run from source.

```bash
# 1) Toolchains (skip any you already have)
brew install rust node pnpm ollama
xcode-select --install

# 2) Start Ollama + pull a small model to gate
ollama serve &
ollama pull llama3.2:1b

# 3) Clone, install, run
git clone https://github.com/QuantaMinds/QuantaMind.git
cd QuantaMind/frontend
pnpm install
pnpm tauri dev            # first build is slow; opens a native window
```

Open the **Tests** tab, pick your model, run a built-in agentic collection for your first verdict — then check **Agent Report** for the per-model breakdown.

> 💬 Hit a snag? [Open an issue](https://github.com/QuantaMinds/QuantaMind/issues) — frictionless setup is a goal, so setup bugs are real bugs.

<details>
<summary><b>Prerequisites & optional backends</b></summary>

| Tool | Version | Required? |
|---|---|---|
| **Rust** | 1.75+ | required |
| **Node** | 20+ | required |
| **pnpm** | 9+ | required |
| **Ollama** | latest | required — the default backend |
| **llama.cpp** (`llama-server`) | latest | optional — run GGUF models directly |
| **MLX** (`pip install mlx-lm`) | latest | optional — Apple Silicon only |
| **whisper.cpp** | latest | optional — speech-to-text (`brew install whisper-cpp`) |

</details>

<details>
<summary><b>Windows dev shell</b> — one-time setup for <code>pnpm tauri dev</code></summary>

On Windows, source two things into your PowerShell session before `cargo` (and therefore `pnpm tauri dev`) can run: **the Rust bin dir on PATH** (rustup only updates *new* shells) and the **MSVC linker env** from Visual Studio's `vcvars64.bat`. macOS and Linux need neither.

If you hit `program not found: cargo metadata` or `linker link.exe not found`:

```powershell
# Refresh PATH for this session (already permanent on the User env var).
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:APPDATA\npm;$env:LOCALAPPDATA\Programs\Ollama;$env:Path"

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

---

## What's inside

QuantaMind is a workbench, not a chat app — each surface answers one question about a local model.

| Tab | What it's for |
|---|---|
| 🧪 **Tests** | Score models on tiered agentic scenarios (Easy→Extreme) with pass^k reliability, failure-mode classification, and a visual trace debugger. |
| 📋 **Agent Report** | Per-model **Ready / Conditional / Not Ready** verdict, tier-progression matrix, failure taxonomy, and an opt-in community leaderboard. |
| ⌨️ **Workspace** | Monaco prompt editor with token-by-token streaming, per-run metrics (TTFT, tok/s), and YAML save/load. |
| 🎙️ **Speech-to-Text** | Fully local transcription via whisper.cpp, with an optional voice → assistant pipeline. |
| 📦 **Models** | Install from Ollama Library, Hugging Face, or a local `.gguf`; disk-safe, resumable, with a storage manager. |
| 📊 **Analysis** | Multi-model compare and quantization diffing, with throughput/TTFT charts and Markdown/JSON export. |

![The Tests scoreboard: a batch run scoring a 100% pass rate on Easy-tier agentic tasks, with a per-model Pass^k summary below.](docs/screenshots/tests-scoreboard.png)

<details>
<summary><b>The details behind each tab</b></summary>

**Tests** — Graduated tiered scenarios spanning coding, finance, medical, legal, ecommerce, support, supply-chain, math/science, and clinical-trial domains. Deterministic sandbox-free scoring (tool-call accuracy, pass^k, avg steps, schema resilience, dominant failure mode). A **Difficulty Tier** control recommends pass^k iterations and step budgets per tier; an **Anti-Saturation** toggle injects decoy tools to resist contamination. Failure modes are named honestly — `ForbiddenCall`, `LoopCap`, `FakeDone`, `foreign_dialect`, `empty_output`, `Reasoning-overrun` — never just pass/fail. A **Thinking-model** flag (auto-detected) raises the token budget and strips `<think>` scratchpads so a reasoner isn't scored as malformed. A **Context Stress Test** finds the prompt length where tool-call accuracy collapses. Deterministic environments let a task run against a **simulated filesystem** the agent browses with `read_file`/`list_dir`/`grep`, replayed visually in a step-scrubber.

**Agent Report** — Verdict gated on your configurable readiness profile (min pass^k, forbid loops/false-done, require full VRAM, min context, require native FC). Both tool-calling paths (Native FC / Prompt-Based) get a separate verdict row. Hardware-aware VRAM fit (exact weights + KV cache vs an allocation cap). A per-model deep-dive shows an Executive Verdict, Tier Progression Matrix, and Failure Taxonomy. Export as standalone HTML, or publish a verdicts-and-metrics-only row (opt-in, default-off — never prompts, traces, or machine identifiers).

**Workspace** — Live model picker driven by `/api/tags`, explicit `running`/`streaming`/`done`/`cancelled`/`error` states, clean stream cancellation (no fake "done"), byte-identical YAML round-trip, and a persistent status bar with Ollama health.

**Speech-to-Text** — whisper.cpp (`whisper-server`) on a fixed `:8093`, its own engine axis parallel to the LLM backend. Curated model catalog, atomic both-or-none installs, audio decoded/downmixed/resampled to 16 kHz in Rust. Offline-only by construction — a loopback-only probe means transcription never silently reaches the cloud.

**Models** — One modal, three sources. Disk pre-check refuses any install leaving < 2 GB free. Real-time progress (bytes/speed/ETA), cancellable, resumable HF downloads (`.partial` + Range), pure-Rust GGUF header parsing, and an 8-family chat-template registry.

**Analysis** — Multi-select models, one prompt, three run strategies with a hardware feasibility verdict (`ok`/`risky`/`wont_fit`) computed at click time. Per-model streaming columns, throughput/TTFT chart, word-level diff, and a **Quant** sub-tab that compares a family's quantizations on size · fit · quality · tool-calling.

</details>

![The Agent Report tab: the Local Agent Readiness Validator with host thresholds, a per-model verdict row, and an Executive Verdict reading READY.](docs/screenshots/agent-report.png)

---

## Roadmap

- **Windows + Linux** desktop builds — *in progress*. The platform-adapter foundation has landed; sidecar lifecycles, multi-vendor GPU probe, and native storage paths are being rewired phase-by-phase.
- **WebGPU** — run the readiness gate in the browser, on your own GPU, zero install.
- **Expanded task suite** — more agentic tiers and domains.
- **Deterministic visual environments** — stateful WebUI + vision/OCR readiness evals.

---

## Tech stack

Tauri 2.x + Rust + React 18 + TypeScript 5 + Vite + Tailwind + Zustand. These choices are **locked** — substitutions require explicit review.

<details>
<summary><b>Full dependency table</b></summary>

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | **Tauri 2.x** | ~30 MB binaries, native WebView, Rust backend |
| Backend language | **Rust 1.75+** (ed. 2021) | Tauri default; safe IPC + HTTP |
| Frontend | **React 18 + TypeScript 5** | Largest open-source contributor pool |
| Build tool | **Vite 5** | Fast HMR, Tauri-friendly |
| Styling | **Tailwind CSS 3** | Utility-first, no design-system overhead |
| State | **Zustand** | ~1 KB, no boilerplate, scales |
| Editor | **`@monaco-editor/react`** | Same editor as VS Code |
| HTTP client (Rust) | **`reqwest` + `tokio`** | Battle-tested |
| Speech-to-text | **whisper.cpp** (`whisper-server` sidecar) | Local STT over HTTP on `:8093`; subprocess, not FFI |
| Audio preprocessing | **`hound` + `rubato`** | Decode WAV → downmix → resample to 16 kHz in-process |
| Voice-activity detection | **`webrtc-vad`** | Independent non-ML VAD for the silence-hallucination metric |
| Serialization | **`serde` / `serde_json` / `serde_yaml`** | Type-safe across IPC |
| Validation | **`zod`** (TS) · **`validator` + `serde`** (Rust) | Runtime schema validation at the IPC boundary |
| Testing | **`cargo test` + `mockito`** · **`vitest` + Testing Library** | Built-in, fast |
| Format / Lint | **`rustfmt` + Clippy** · **Prettier + ESLint** | Auto-format on save |
| Pre-commit / CI | **`lefthook`** · **GitHub Actions** | Lighter than Husky; free for OSS |

**Deliberately not installed (yet):** no logging library, no state-machine library, no UI component library, no form library, no in-process AI/ML libraries. Every dependency is a maintenance debt.

</details>

---

## Architecture

```
┌──────────────────────────────────────────────┐
│           React + TypeScript Frontend        │
│    features/  ←  shared/ipc/  ←  invoke()    │
└─────────────────────┬────────────────────────┘
                      │  IPC boundary (JSON)
┌─────────────────────▼────────────────────────┐
│              Rust Backend (backend/)         │
│   commands/  →  inference/  →  metrics/      │
│                    ↓                          │
│               persistence/                    │
└─────────────────────┬────────────────────────┘
                      │  HTTP
                      ▼
          Ollama · llama.cpp · MLX  (local)
```

The two halves talk JSON over Tauri's IPC — contracts explicit in `shared/ipc/types.ts`, mirrored in Rust, no codegen. `shared/ipc/` is the **only** place that calls `invoke`. Each file is single-concern; each module owns one responsibility.

> **New here?** [`ARCHITECTURE.md`](ARCHITECTURE.md) is the five-minute map of the backend's hexagonal layout and the one dependency rule that keeps it navigable.

---

## Building from source

```bash
cd frontend
pnpm install
pnpm tauri dev            # development, with HMR
pnpm tauri build          # production → backend/target/release/bundle/ (.dmg + .app)
```

macOS is the shipping target today; Windows and Linux are being rewired phase-by-phase (see [Roadmap](#roadmap)).

```bash
cd frontend && pnpm test          # frontend (vitest)
cd backend  && cargo test         # backend (cargo test + mockito)
```

---

## Documentation

The README stays lean on purpose. Depth lives in `docs/`:

- **[`docs/codebase/`](./docs/codebase/README.md)** — deep, file-by-file reference for every backend module and frontend page (**Why** it exists, **What** it does, **How/Where** it's used). Jump to [inference engines](./docs/codebase/backend-inference-backends.md), [the eval engine](./docs/codebase/backend-eval-engine.md), [STT](./docs/codebase/backend-stt.md), or [the Workspace tab](./docs/codebase/frontend-workspace.md).
- **[`docs/architecture.md`](./docs/architecture.md)** — modules, IPC, layering law, robustness rules, folder taxonomy.
- **[`docs/process.md`](./docs/process.md)** — tech stack, setup, conventions, the step-by-step workflow, roadmap.
- **[`docs/reference.md`](./docs/reference.md)** — analysis/bench schema and troubleshooting.

---

## Contributing

Contributions welcome. The engineering principles are non-negotiable — start with [`CLAUDE.md`](./CLAUDE.md) and [`docs/process.md#workflow`](./docs/process.md#workflow).

**Before you open a PR:**

- [ ] Single concern (one feature, bug, or refactor)
- [ ] Each file stays single-concern — split by responsibility, not line count
- [ ] Tests added/updated and passing — *and outputs verified* (a green CI run is necessary, not sufficient)
- [ ] Docs in `docs/` updated in the same PR
- [ ] No `unwrap()` outside tests
- [ ] Commits follow [Conventional Commits](https://www.conventionalcommits.org/); branches are `<type>/<short-description>` (`feature/`, `fix/`, `bug/`, `docs/`, `chore/`, `refactor/`)

---

## Security & privacy

> [!IMPORTANT]
> QuantaMind is local-first by design.

- **No telemetry, no account** — no analytics SDK, no crash reporting, no tracking. Runs offline once a model is installed.
- **Network calls limited to** local model servers (`localhost:11434` Ollama, `127.0.0.1:8093` whisper.cpp, dynamic llama/MLX ports) and `huggingface.co` (only when you actively browse/install).
- **Speech-to-text is offline-only** — a loopback-only probe; a down local server fails loud rather than silently falling back.
- **No silent shell edits** — changing `OLLAMA_MODELS` *generates* the export command; it never edits your shell profile.
- **Tauri sandboxing** — the webview can only call IPC commands explicitly registered in `backend/capabilities/`.
- **Schema validation at every IPC boundary** — Zod on TS, serde + `validator` on Rust; malformed payloads rejected with typed errors.

Found a vulnerability? Please open a [private security advisory](https://github.com/QuantaMinds/QuantaMind/security/advisories/new) instead of a public issue.

---

## FAQ

<details>
<summary><b>Is QuantaMind a chat app?</b></summary>

No — it's a workbench. Each Workspace run is a single prompt → single completion. The multi-step **agentic** loops live in the eval engine, not a chat UI.

</details>

<details>
<summary><b>Does it fine-tune or train models?</b></summary>

No. QuantaMind consumes pre-trained models; training is out of scope.

</details>

<details>
<summary><b>Why Ollama and not llama.cpp directly?</b></summary>

Ollama gives a clean HTTP API, a stable storage convention, and handles GPU plumbing. It's no longer the only backend, though — llama.cpp (`llama-server`) and MLX (`mlx_lm`, Apple Silicon) adapters ship alongside it, all behind one `InferenceBackend` trait.

</details>

<details>
<summary><b>Does my audio leave the machine?</b></summary>

No. Speech-to-text runs entirely on local whisper.cpp (`127.0.0.1:8093`). Audio is decoded and resampled in Rust and sent only to the local server; a loopback-only probe means it never reaches the cloud.

</details>

<details>
<summary><b>Can I run without an internet connection?</b></summary>

Yes, once you've installed at least one model. Workspace, Voice, and Analysis are fully offline. Only the Hugging Face tab — and downloading new models — needs connectivity.

</details>

<details>
<summary><b>Does it send any usage data?</b></summary>

None. The only outbound HTTP is to your local Ollama and (when you ask) to Hugging Face.

</details>

---

## License & acknowledgements

Apache 2.0 — see [`LICENSE`](./LICENSE).

Built on [Tauri](https://tauri.app/), [Ollama](https://ollama.com/), [llama.cpp](https://github.com/ggerganov/llama.cpp), [whisper.cpp](https://github.com/ggerganov/whisper.cpp), [Hugging Face](https://huggingface.co/), [Monaco Editor](https://microsoft.github.io/monaco-editor/), and the [React](https://react.dev/) / [Vite](https://vitejs.dev/) / [Tailwind](https://tailwindcss.com/) / [Zustand](https://github.com/pmndrs/zustand) stack — plus the open-weights model communities (Meta, Mistral, Qwen, Microsoft, Google, DeepSeek, and many others).

<div align="center">
<br/>
<sub><b>Built with discipline. Local-first by design.</b> · Made by QuantaMind</sub>
</div>

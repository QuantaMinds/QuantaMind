<div align="center">

# QuantaMind

**The pre-deployment gate for local AI agents.**

Benchmark any **llama.cpp**, **llama.cpp**, or **vLLM** model for *agentic readiness* on your own hardware — and get a **Ready / Conditional / Not Ready** verdict before you wire it into an agent. Runs fully local by default (nothing leaves the machine); optionally point it at a **remote vLLM** GPU server when you need to bench a model bigger than your box.

<sub>Local-first · No telemetry · No account · pass^k scoring · hardware-aware · one ~30 MB binary</sub>

<br/>

[![Discord](https://img.shields.io/badge/Discord-Get%20help%20from%20the%20team-5865F2?logo=discord&logoColor=white)](https://discord.gg/qN5uTn9KKA)
[![Repo](https://img.shields.io/badge/GitHub-QuantaMinds%2FQuantaMind-181717?logo=github)](https://github.com/QuantaMinds/QuantaMind)
[![Website](https://img.shields.io/badge/Website-quantamind.co-2563EB?logo=googlechrome&logoColor=white)](https://quantamind.co/)

![Version](https://img.shields.io/badge/version-0.2.4-blue)
![Platform](https://img.shields.io/badge/platform-macOS-blue)
![License](https://img.shields.io/badge/license-Apache%202.0-green)

<br/>

![QuantaMind — the readiness gate for local AI agents: qualify the exact model, quantization, runtime, and hardware you will deploy, and get one answer: Ready, Conditional, or Not Ready. Fully offline.](docs/screenshots/hero.png)

</div>

---

## ⚡ Quick start (no build required)

You do **not** need Rust, Node, or any toolchain to use QuantaMind.

### Option 1 · Desktop app

Download the prebuilt app from **[quantamind.co](https://quantamind.co/)** and launch.

### Option 2 · The `qm` CLI (headless)

Prefer the terminal? Install in seconds:

```bash
# macOS / Linux
curl -fsSL https://github.com/QuantaMinds/QuantaMind/releases/latest/download/quantamind-installer.sh | sh
```

```powershell
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/QuantaMinds/QuantaMind/releases/latest/download/quantamind-installer.ps1 | iex"
```

Prebuilt for macOS (Apple Silicon + Intel), Linux (x64 + arm64, plus a fully static musl build for containers), and Windows x64 — every artifact checksummed and attestation-signed.

### Your first verdict

QuantaMind drives a model you already run locally — so you need a backend up (the default is [llama.cpp](https://github.com/ggml-org/llama.cpp)):

```bash
# 1. Start a backend and pull a small model to gate
llama-server -m llama3.2:1b

# 2. Probe your backends — every failure prints its exact fix
qm doctor

# 3. Zero-config: pick a backend+model and run a first verdict
qm init
```

In the **desktop app**: open the **Tests** tab, pick your model, run a built-in agentic collection — then check **Agent Report** for the verdict.

Full CLI reference, container images, checksums + attestation verification: **[CLI quickstart](./docs/cli/README.md#quickstart--three-commands-to-your-first-verdict)**.

> 💬 **Hit a snag?** [Troubleshooting](./docs/reference.md#troubleshooting) covers the common ones — including macOS's ["developer cannot be verified" dialog](./docs/reference.md#macos-gatekeeper). Setup bugs are real bugs: [open an issue](https://github.com/QuantaMinds/QuantaMind/issues) or ask in [Discord](https://discord.gg/qN5uTn9KKA).

---

## 🧠 Why QuantaMind?

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

## What's inside

QuantaMind is a workbench, not a chat app — each surface answers one question about a local model.

| Tab | What it's for |
|---|---|
| 🧪 **Tests** | Score models on tiered agentic scenarios (Easy→Extreme) with pass^k reliability, failure-mode classification, and a visual trace debugger. Includes a **Safety & Boundaries (Category K)** axis: prompt-injection resistance + an over-refusal control, a compliance gate, and model-vs-config attribution of a safety failure. Also reports **Tokens/Task** (amortized cost incl. failed-run waste) and supports **payload-noise** scenarios that test field extraction from messy real-world tool JSON. |
| 📋 **Agent Report** | Per-model **Ready / Conditional / Not Ready** verdict, tier-progression matrix, failure taxonomy, and an opt-in community leaderboard. |
| ⌨️ **Workspace** | Monaco prompt editor with token-by-token streaming, per-run metrics (TTFT, tok/s), and YAML save/load. |
| 📦 **Models** | Install from llama.cpp Library, Hugging Face, or a local `.gguf`; disk-safe, resumable, with a storage manager. |
| 📊 **Analysis & Latency** | Multi-model compare and quantization diffing, with throughput/TTFT charts and Markdown/JSON export. |
| 📖 **Docs** | Built-in, task-oriented user guides (getting started, choosing a backend, running tests, troubleshooting) with ⌘K full-text search. |

![The Tests scoreboard: a batch run scoring a 100% pass rate on Easy-tier agentic tasks, with a per-model Pass^k summary below.](docs/screenshots/tests-scoreboard.png)

<details>
<summary><b>The details behind each tab</b></summary>

**Tests**
- Tiered scenarios (Easy→Extreme) across nine domains, with a coding·finance·medical spine at every tier so a model stays comparable as difficulty rises.
- Deterministic scoring: tool-call accuracy, pass^k, schema resilience — and failure modes named honestly (`ForbiddenCall`, `LoopCap`, `FakeDone`, …), never just pass/fail.
- Run a whole collection or click one task to run just it; every lever (k, step caps, decoy tools, thinking budget, native/prompt method) applies unchanged.
- A Context Stress Test finds where tool-call accuracy collapses — and only ever reports a depth it actually measured. Reasoning models get a Lean/Standard/Deep thinking budget that scales with each rung's depth (same table as the test tiers).
- Custom JSON collections are validated before import, with per-task fix-it messages — a fixture bug can't masquerade as "every model fails my task".

**Agent Report**
- Verdict gated on a readiness profile you configure: min pass^k, forbid loops/false-done, VRAM fit, min context, native function calling.
- Separate verdict rows for native and prompt-based tool calling; VRAM fit graded at the KV-cache precision the launch would actually use.
- Right-Sizing names the smallest quant of each family still Ready on your hardware, with measured deltas (size · memory · pass^k).
- Export standalone HTML, or publish an opt-in verdicts-and-metrics-only row; an "Equivalent CLI command" preview reproduces the exact same bar headlessly.

**Workspace** — live model picker, explicit run states with clean cancellation (no fake "done"), byte-identical YAML round-trip, persistent backend-health status bar.

**Models** — one modal, three sources (llama.cpp Library / Hugging Face / local `.gguf`); disk pre-check, cancellable resumable downloads, pure-Rust GGUF parsing.

**Analysis & Latency**
- Multi-model compare with a hardware feasibility verdict (`ok`/`risky`/`wont_fit`) at click time, and the full per-token latency breakdown under each answer.
- Context-ceiling meters by KV-cache precision (f16 / q8_0 / q4_0), budgeted on the **measured** Metal working-set limit on Apple Silicon, with a `Fits`/`Tight`/`SpillsToCpu` verdict.
- The Latency tab doubles as the Tests cost page: per-task prefill/decode split, thinking tokens, cache hits, KV memory at peak context, and OOM forensics — every number labeled measured, computed, estimated, or claimed.

The full tour of every control and caveat lives in the in-app **Docs** tab and [`docs/codebase/`](./docs/codebase/README.md).

</details>

![The Agent Report tab: the Local Agent Readiness Validator with host thresholds, a per-model verdict row, and an Executive Verdict reading READY.](docs/screenshots/agent-report.png)

---

## 🛠 CI & pipeline integration

Everything the desktop app measures, headless — for CI gates, SSH boxes, air-gapped runs, and scripts. The whole journey is five commands:

```console
$ qm doctor        # connect: probes all 5 backends, prints the exact fix for anything broken
$ qm init          # zero-config: picks a backend+model, writes qm.json, runs a first verdict
$ qm validate --collection <id|file.json>   # prove a test is trustworthy BEFORE spending a model on it
$ qm run --k 8 --costs --save-report r.json --junit j.xml --save-transcripts traces/
$ qm report --report r.json --profile coding-agent   # re-score offline under any team policy
```

The verdict **is** the exit code — `0` Ready · `10` Conditional · `20` NotReady · `11` Inconclusive (infra, retry) — so a pipeline gates on it directly, and the bundled **[`qm-eval` GitHub Action](./docs/ci/README.md)** (prebuilt binary, no toolchain) or the **`ghcr.io/quantaminds/qm`** container does it for you. Every number is measured or `n/a`, never estimated: per-task steps, thinking tokens (tokenized split on llama.cpp), cache hits, peak context, per-task KV, RSS.

Full reference: **[docs/cli](./docs/cli/README.md)**.

---

## Roadmap

- **Windows + Linux** desktop builds — *in progress*. The platform-adapter foundation has landed; sidecar lifecycles, multi-vendor GPU probe, and native storage paths are being rewired phase-by-phase.
- **WebGPU** — run the readiness gate in the browser, on your own GPU, zero install.
- **Expanded task suite** — more agentic tiers and domains.
- **Deterministic visual environments** — stateful WebUI + vision/OCR readiness evals.
- **Category K, broadened** — more injection vectors (sleeper, contradictory params, parameter validation), a visual authoring toggle for custom safety probes, and a redacted auditor-evidence export.

---

## 📖 Documentation & contributing

QuantaMind is an open-source workbench — Tauri 2.x + Rust + React 19 + TypeScript 5 + Vite + Tailwind + Zustand (the stack is **locked**; full table in [`docs/process.md#tech-stack`](./docs/process.md#tech-stack)). If you want to build the desktop app from source, dive into the architecture, or add new agentic tests:

- **[Building from source](./CONTRIBUTING.md#project-setup)** — Rust/Node/pnpm toolchains for macOS, Linux, and Windows (incl. the Windows dev-shell setup), plus the dev/test loop. ~5 minutes on macOS.
- **[Contribution guidelines](./CONTRIBUTING.md)** — ground rules, branching, the PR checklist.
- **[`ARCHITECTURE.md`](./ARCHITECTURE.md)** — the five-minute map: React frontend ↔ JSON-over-IPC ↔ Rust backend ↔ HTTP to llama.cpp / vLLM (local) or vLLM (remote GPU).
- **[`docs/codebase/`](./docs/codebase/README.md)** — deep, file-by-file reference for every backend module and frontend page.
- **[`docs/architecture.md`](./docs/architecture.md)** — modules, IPC, layering law, robustness rules, folder taxonomy.
- **[`docs/process.md`](./docs/process.md)** — tech stack, setup, conventions, the step-by-step workflow, roadmap.
- **[`docs/reference.md`](./docs/reference.md)** — analysis/bench schema, adding custom tests, troubleshooting.
- **[`docs/cli/README.md`](./docs/cli/README.md)** — the headless `qm` CLI, end to end.

---

## Security & privacy

> [!IMPORTANT]
> QuantaMind is local-first by design.

- **No telemetry, no account** — no analytics SDK, no crash reporting, no tracking. Runs offline once a model is installed.
- **Network calls limited to** local model servers (`localhost:8081` llama.cpp, dynamic llama/vLLM ports) and `huggingface.co` (only when you actively browse/install). The one exception is **opt-in**: if you configure a remote vLLM server in Settings, prompts you run on that backend are sent to the URL you entered (empty by default).
- **No silent shell edits** — changing `QUANTAMIND_GGUF_DIR` *generates* the export command; it never edits your shell profile.
- **Tauri sandboxing** — the webview can only call IPC commands explicitly registered in `backend/capabilities/`.
- **Schema validation at every IPC boundary** — Zod on TS, serde + `validator` on Rust; malformed payloads rejected with typed errors.

Found a vulnerability? Please open a [private security advisory](https://github.com/QuantaMinds/QuantaMind/security/advisories/new) instead of a public issue.

---

## Community

- ⭐ [Star the repo](https://github.com/QuantaMinds/QuantaMind) — if QuantaMind is useful
  to you, a star is the single best way to help others find it
- 💬 [Discord](https://discord.gg/qN5uTn9KKA) — talk directly to the core team
  (also one click away in the app — the **Discord** button in the header)
- 🐦 [𝕏 (Twitter)](https://x.com/QuantaMind_2025)
- 🐛 [Issues](https://github.com/QuantaMinds/QuantaMind/issues) — setup bugs are real bugs

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
<summary><b>Why llama.cpp and not llama.cpp directly?</b></summary>

llama.cpp gives a clean HTTP API, a stable storage convention, and handles GPU plumbing. It's no longer the only backend, though — llama.cpp (`llama-server`) and vLLM (`vllm_lm`, Apple Silicon) run locally, and vLLM connect to a remote OpenAI-compatible GPU server (URL + optional API key set in Settings) — all behind one `InferenceBackend` trait.

</details>

<details>
<summary><b>Can I run without an internet connection?</b></summary>

Yes, once you've installed at least one model. Workspace, Tests, and Analysis are fully offline. Only the Hugging Face tab — and downloading new models — needs connectivity.

</details>

<details>
<summary><b>Does it send any usage data?</b></summary>

None. The only outbound HTTP is to your local llama.cpp and (when you ask) to Hugging Face.

</details>

---

## License & acknowledgements

Apache 2.0 — see [`LICENSE`](./LICENSE).

Built on [Tauri](https://tauri.app/), [llama.cpp](https://github.com/ggml-org/llama.cpp), [llama.cpp](https://github.com/ggerganov/llama.cpp), [Hugging Face](https://huggingface.co/), [Monaco Editor](https://microsoft.github.io/monaco-editor/), and the [React](https://react.dev/) / [Vite](https://vitejs.dev/) / [Tailwind](https://tailwindcss.com/) / [Zustand](https://github.com/pmndrs/zustand) stack — plus the open-weights model communities (Meta, Mistral, Qwen, Microsoft, Google, DeepSeek, and many others).

<div align="center">
<br/>
<sub><b>Built with discipline. Local-first by design.</b> · Made by QuantaMind</sub>
</div>

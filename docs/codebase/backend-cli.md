# Backend — The `qm` headless CLI

> Subsystem doc. Scope: `src/bin/qm/` (the thin binary) and `src/cli/` (the pure
> engines). The CLI is the desktop app's engine driven from a terminal — same
> scoring, same verdicts, same honesty rules — with a documented exit-code
> contract a CI pipeline gates on.

## Overview

**Why this exists.** The eval engine's verdicts are only useful in automation if
they arrive without a GUI: first-run setup over SSH, CI gates, air-gapped runs,
scripted sweeps. `qm` is that face. It is **headless by construction**: the crate's
`gui` cargo feature (default-on) compiles the entire Tauri/desktop dependency tree
out of the CLI build, so release binaries run on bare servers and containers
(see `docs/architecture.md#the-gui-feature-seam-headless-cli-build`).

**Design law (thin bin, pure core).** `src/bin/qm/main.rs` only parses args
(clap, `QM_BASE`/`QM_MODEL`/`QM_API_KEY` env-backed), calls a pure engine in
`src/cli/`, renders, and maps exit codes. Stream discipline is absolute: the
**report goes to stdout** (so `qm run --json | jq` is never polluted), every
diagnostic/progress/`[QM-…]` fix line goes to **stderr**.

**The exit-code contract** (what CI gates on): `0` Ready · `10` Conditional ·
`20` NotReady · `11` Inconclusive (infra fault — retry, never a fake verdict) ·
`2` bad args/capability mismatch · `3` nothing runnable. `--fail-on` is the
policy knob; `.github/actions/qm-eval` wires it into pipelines.

## Module map — `src/cli/`

| Module | Why / What |
|---|---|
| `doctor/` | The connect probe. `probe/` holds per-backend candidate-URL probes (Ollama, llama.cpp, MLX, vLLM/SGLang; `--base`/`QM_BASE` always wins); `report.rs` decides **runnable** (reachable + ≥1 model + credential OK — the full `RemoteAuthStatus` failure space, so a bad key is never conflated with a down server); `render.rs` prints the table + the exact fix command per failure. |
| `init/` | Zero-config first verdict: auto-detect a runnable backend, pick a model, write `./qm.json` (`run/config.rs`), run the suite once. The `gh auth login` moment. |
| `run/` | The heart. `mod.rs` orchestrates: load collection (built-in id or file) → validate (the same trust boundary as import) → probe native eligibility → drive the shared batch engine → assess against the readiness profile → render verdict. `sink.rs` is the CLI `BatchSink` (progress to stderr; captures per-step `TrajectoryStep`s when `--costs` or `--save-transcripts` asks). `render.rs` prints the verdict block (pass^k, runs, **avg steps**) and the `--costs` table (per-task prefill/decode, thinking tokens — measured split on llama.cpp, `n/a` on Ollama, never estimated — cache hits, peak context, **per-task `kv@peak`**, RSS). `costs.rs` assembles rows + memory facts with provenance labels; `junit.rs` emits the CI test panel; `config.rs` round-trips `qm.json`. |
| `validate.rs` | `qm validate` — prove a collection/world is a trustworthy test BEFORE spending a model: oracle reachability, do-nothing discrimination, and the semantic authoring contract (`oracle::semantic_findings` — prompt-leaked facts, cross-entity duplicates, fault-placement hazards, leaky getters, ungrounded answer tokens). Same implementation as the GUI import gate and MCP world validation, so the surfaces can never drift. |
| `cliff.rs` | `qm cliff` — the Context Stress Test (padding-ladder accuracy collapse), CLI twin of the Audit tab. |
| `prompt.rs` | `qm prompt` — free-form generation through the same backend dispatch (Workspace's twin). |
| `costs.rs` + `costs_cli_tests.rs` | `qm costs <collection>` — re-render a saved run's cost rows offline; no model, no server. |

## `src/bin/qm/main.rs`

Clap command tree (`doctor`/`init`/`run`/`test`/`validate`/`cliff`/`report`/
`prompt`/`costs`), interactive pickers only when stdout is a TTY (never prompts in
CI/SSH), and the flag surface: `--save-report` (raw report JSON for offline
`qm report`), `--junit`, `--costs`, `--save-transcripts <dir>` (per-step
trajectory JSONL — the same format the desktop app's `agentic_transcripts` store
uses, for post-mortems without a verbose server log). Secrets discipline: an API
key is NEVER an argument (args are logged); `QM_API_KEY` env or OS keychain only.

## How it stays honest (the rules this module enforces)

- Every number is **measured or `n/a`** — thinking tokens print `(tokenized split)`
  only when llama.cpp's `/tokenize` measured the split; Ollama prints `n/a`
  (no tokenize endpoint), never a chars÷4 estimate. KV figures are formula-from-
  measured-dims, gated to `n/a` when dims are unmeasurable.
- Infra faults are `11 Inconclusive` ("retry"), never a `NotReady` verdict —
  a dead server must not read as a bad model.
- `run`/`test` apply the same `validate` gate automatically to uploaded files.

## How/Where used

`docs/cli/README.md` is the user-facing reference; `.github/actions/qm-eval`
wraps `qm run` for CI (prebuilt-binary install by default);
`ghcr.io/quantaminds/qm` ships the same binary for container pipelines. The
release pipeline (`quantamind-release.yml`, `quantamind-v*` tags) builds it for
six targets via the lean `--no-default-features` seam.

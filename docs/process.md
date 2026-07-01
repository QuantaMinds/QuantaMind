# Working on QuantaMind

The locked tech stack, day-zero setup, conventions, the mandatory per-step
workflow, the data-quality gate, the phase roadmap, and deferred ideas.
Companion docs: `architecture.md` and `reference.md`.

## Tech stack

Locked decisions. Do not substitute. Alternatives go to
[Future considerations](#future-considerations).

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | Tauri 2.x | 30MB binary, native WebView, Rust backend |
| Backend language | Rust (stable, ed. 2021) | Tauri default, safe IPC + HTTP |
| Frontend framework | React 18 + TS 5.x | Largest open-source contributor pool |
| Build tool | Vite 5.x | Fast HMR, Tauri-friendly |
| Styling | Tailwind CSS 3.x | Utility-first, no design-system overhead |
| State management | Zustand | 1KB, no boilerplate, scales |
| Editor component | `@monaco-editor/react` | Same editor as VS Code |
| HTTP client (Rust) | `reqwest` + `tokio` | Standard, battle-tested |
| Serialization | `serde` + `serde_json` / native JSON | Type-safe across IPC |
| Validation (TS) | `zod` | Runtime schema validation |
| Validation (Rust) | `validator` + `serde` | Type-level + custom validators |
| Testing (Rust) | `cargo test` + `mockito` | Built-in, no setup |
| Testing (TS) | `vitest` + `@testing-library/react` | Fast, Vite-native |
| E2E | Playwright | Cross-platform |
| CI | GitHub Actions | Free for open source |
| Format | `rustfmt` + Prettier | Auto-format on save |
| Lint | Clippy + ESLint | Catch problems pre-runtime |
| Pre-commit | lefthook | Lighter than Husky |

Phase 3 additions (locked; installed when their step lands):

| Layer | Choice | Why |
|---|---|---|
| Word diff (TS) | `diff-match-patch` | De-facto standard for word-level diffs; tiny, dependency-free. |
| Secret storage (Rust) | `keyring` | OS-native keychain for cloud API keys — never plaintext on disk. |
| llama.cpp backend | `llama-server` (Tauri sidecar binary) | Local GGUF inference over HTTP, mirroring the Ollama path. Subprocess, not in-process FFI. |
| STT engine (Phase 0) | `whisper-server` (whisper.cpp `server` example; bundled sidecar) | Local speech-to-text over HTTP on `:8093`, mirroring the `llama-server` lifecycle; reuses the bundled `libggml-*` dylibs. Subprocess, not FFI. State-aware `/health`; silero VAD bundled with each model. |
| STT audio preprocessing (P1/P2) | `hound` (WAV) + `symphonia` (MP3/others, P2 upload) + `rubato` (resample) + `reqwest` `multipart` | Decode audio → downmix → resample to 16 kHz mono **in Rust** (explicit, logged) → POST per ~30 s window to whisper-server `/inference` (`verbose_json`). Pure-Rust, not FFI; lets the seam stream + bound memory + assert the resample/duration in our own code. WAV-first; `symphonia` (compressed formats) deferred. |
| STT Inspector VAD (P3) | `webrtc-vad` | The silence-hallucination metric needs an **independent** voice-activity detector over the raw 16 kHz PCM — never the STT model's own opinion, or the metric is circular. WebRTC VAD is deterministic, non-ML (energy/SBC), negligible CPU, loads no model. Its `Vad` C handle is `!Send`, so the profiling fold runs on a `spawn_blocking` thread. Distinct from whisper-server's bundled silero VAD (which lives inside the decode path). |

Phase 4 additions (locked; installed when their step lands):

| Layer | Choice | Why |
|---|---|---|
| Charting (TS) | `visx` v4 (`@next`, scale/shape/group) | React-19-native (stable v3 pins React ≤18). Modular SVG primitives for the Latency charts; we draw axes/legends ourselves. Pin the alpha until v4 ships stable. |

### What is explicitly NOT installed (yet)

- Logging library — use `println!` / `console.log` until Phase 2.
- State-machine library — Zustand is enough.
- UI component library — Tailwind utility classes only.
- Form library — no forms in Phase 1.
- AI/ML libraries — we do not run models in-process. We call Ollama, and a
  bundled `llama-server` sidecar over HTTP — never FFI.

Resist additions. Every dependency is a maintenance debt. Update this section
only when a locked choice is replaced (requires a PR with rationale) or a new
layer is added.

---

## Setup

Run these in order. Each is a checkpoint — do not paste them all at once.
macOS shown; adapt for Linux/Windows.

```sh
# 1. Prerequisites
brew install rust node pnpm
rustc --version    # 1.75+
node --version     # 20+
pnpm --version     # 9+
xcode-select --install

# 2. Create the project
pnpm create tauri-app@latest
# Project name: quantamind · Identifier: dev.quantamind.app
# Frontend: TypeScript · Package manager: pnpm · UI template: React (TS)
cd quantamind

# 3. Install frontend dependencies
pnpm add zustand zod @monaco-editor/react react-router-dom
pnpm add -D tailwindcss@3 postcss autoprefixer \
  vitest @testing-library/react @testing-library/jest-dom \
  @types/react jsdom
# (Rust deps are added in Phase 1, step 1.)

# 4. Initialize Tailwind
pnpm exec tailwindcss init -p
# tailwind.config.js content: ['./index.html','./src/**/*.{js,ts,jsx,tsx}']
# src/index.css: @tailwind base; @tailwind components; @tailwind utilities;

# 5. Git + pre-commit hooks
git init
git add . && git commit -m "chore: initial Tauri + React + TS scaffold"
pnpm add -D lefthook
pnpm exec lefthook install
# Create lefthook.yml (see CONTRIBUTING.md for content).

# 6. GitHub repo
gh repo create quantamind-dev/quantamind --public --source=. --remote=origin --push
gh repo edit --enable-discussions
# Set up branch protection on main (require PRs).

# 7. Verify the dev loop
pnpm tauri dev
# Edit src/App.tsx, save, see the window reload. If yes → ready.

# 8. Pull Ollama models
brew install ollama
ollama serve &
ollama pull llama3.2:1b       # dev workhorse, ~700MB
ollama pull phi3.5:latest     # variety for later phases
curl http://localhost:11434/api/tags
```

All 8 steps green → development environment ready. Day 1 starts with Phase 1
(see [Phase roadmap](#phase-roadmap)).

---

## Conventions

### Naming

| Domain | Style | Example |
|---|---|---|
| Rust functions / vars | `snake_case` | `run_prompt` |
| Rust types | `PascalCase` | `InferenceBackend` |
| Rust constants | `SCREAMING_SNAKE` | `DEFAULT_TIMEOUT_MS` |
| TS functions / vars | `camelCase` | `runPrompt` |
| TS components / types | `PascalCase` | `PromptEditor` |
| TS constants | `SCREAMING_SNAKE` | `DEFAULT_TIMEOUT_MS` |
| React component file | `PascalCase.tsx` | `PromptEditor.tsx` |
| TS non-component file | `kebab-case.ts` | `use-streaming-run.ts` |
| Rust file | `snake_case.rs` | `ollama.rs` |
| Branch | `<type>/<short-description>` | `feature/streaming-output`, `fix/native-budget`, `bug/empty-output` |

### Commits — Conventional Commits

`feat:` new user-visible behavior · `fix:` bug fix · `chore:` tooling/deps/config
· `docs:` documentation only · `test:` adding/fixing tests · `refactor:` no
behavior change. One step = one commit (or a tight related series). PR title
matches the convention; PR body references "closes #N" when applicable.

### File size

- **Keep each file single-concern.** There is no hard line-count limit; split a
  file when it starts doing *two things*, not when it crosses a number. Splits
  are by responsibility, never by halving. (The ≤10-files-per-folder taxonomy
  rule still holds — see `architecture.md#folder-taxonomy`.)

### Comments

- Default: write none. Naming + structure carry the meaning.
- Write a one-line comment only when the *why* is non-obvious (a workaround, a
  subtle invariant, a constraint not visible in the code).
- Never reference tickets, callers, or "I added this for…".

### Errors

- Rust: `Result<T, AppError>` only. No `unwrap()` outside tests.
- TS: discriminated unions returned across IPC. No thrown errors over the IPC
  boundary.

### Tests

- Live next to code: `__tests__/` in TS, inline `#[cfg(test)]` in Rust,
  integration in `backend/tests/`.
- Name tests after the behavior, not the function: `streams_tokens_in_order`
  not `test_run_prompt`. One behavior per test.

### Colors & imports

- All colors flow through the theme tokens in `frontend/src/styles/tokens.css`.
  Use Tailwind palette classes (token-backed) or the `surface`/`ink` semantic
  colors. No hardcoded hex outside `tokens.css`.
- Absolute imports for cross-feature in TS (`@/shared/...`); relative within a
  feature. Rust: prefer `use crate::module::...` over deep relative paths.

---

## Workflow

The single most important section in the repo. Follow it literally — one step
at a time.

For every unit of work (one step, one ticket, one feature slice):

```
[1] Understand the step
    └─ Read the spec from the roadmap or the user message.
    └─ Write down expected input and expected output BEFORE writing any code.

[2] Implement the minimum
    └─ Smallest code change that could satisfy the spec.
    └─ No speculative abstractions. No "while I'm here" cleanup.

[3] Write the test
    └─ One test per behavior. Name it after the behavior, not the function.
    └─ Tests live next to code: src/.../__tests__/ or backend/tests/.

[4] Run the test
    └─ It must pass. If it does not, fix the code (not the assertion).

[5] Verify the output (DATA QUALITY GATE)
    └─ A green test is NECESSARY but NOT SUFFICIENT.
    └─ Print/log the actual output. Eyeball it.
    └─ Does it match expected shape? types? values? edge cases?
    └─ See the Data quality section for the full checklist.

[6] Update docs
    └─ If behavior changed, update the section that describes it.

[7] Commit
    └─ Conventional Commits. One step = one commit (or a tight series).

[8] Move on
    └─ Only now is the next step allowed to begin.
```

### Common violations (do not do these)

- **Stacking steps.** "Let me knock out steps 1–3 then test." → No. Test after
  each one.
- **Loosening assertions to make tests pass.** If `assert_eq 42` fails because
  the output is 41, do not change to `assert > 40`. Fix the code.
- **Skipping verification because the test passed.** Tests verify the path you
  wrote; verification confirms the path was the right one.
- **Bundling docs into "I'll update them later."** Later does not exist.
- **Refactoring during a feature.** Open a separate branch/commit.

### Stop conditions

Stop and ask the user if a step's spec is ambiguous, the data-quality gate fails
and the fix would change the spec, a file's concern boundary is unclear and the
split is non-obvious, or a test requires hardware that may not be present.

---

## Data quality

A passing test only means the code executed the path you described. Data quality
means the *output* is actually correct. Both gates must pass before a step is
done.

Tests lie when: mocks return canned values matching the assertion but not
reality; assertions check `result != null` but not contents; the function
returns the right type with wrong values; streaming emits tokens in the wrong
order/encoding/count; persistence "succeeds" but writes corrupted YAML.

### The verification checklist

After every green test, run through this:

1. **Shape** — Is the type what you expected (string vs object vs array)? Are
   required fields present, no surprise fields added? For streams: count of
   chunks, ordering, terminator behavior.
2. **Values** — Sample the actual values; in reasonable ranges? Numeric units
   correct (ms vs s, bytes vs MB)? Strings UTF-8, no BOM, no escape leaks?
   Timestamps correct timezone, monotonic where required?
3. **Edge cases** — Empty input → empty output (not crash/null). Very large
   input → handled or rejected with a clear error. Unicode/emoji/RTL → preserved.
   Malformed input → typed error, not a panic.
4. **Cross-boundary fidelity** — Rust → JSON → TS: does the field round-trip
   (snake_case vs camelCase!)? Disk → memory: YAML reloads byte-identical? Partial
   reads / disconnections handled?
5. **Determinism (where applicable)** — Same input → same output? If not, is the
   non-determinism documented?

### How to verify, and when it fails

- Print the output and read it; do not skim. For streams, dump first/last N
  chunks to a file. For persistence, `diff` the round-trip against the original.
  For IPC, log both sides and compare.
- When verification fails: do NOT relax the test — it was right. Fix the
  producing code and add a regression test. If the spec was wrong, update the
  spec AND the test together.
- Logging: verbose `println!` / `console.log` during development is fine; remove
  or gate behind a debug flag before commit. Never commit logs containing prompt
  content or user data.

---

## Phase roadmap

Where each phase begins and ends. Workflow per step is in [Workflow](#workflow)
(implement → test → verify output → docs → commit).

### Phase 1 — v0.1.0 (shipped)

Day-zero usable single-prompt workspace. Tests: 67 backend + 336 frontend
passing.

### Phase 2 — v0.2 Daily-driver polish (complete)

Turns v0.1 into a daily driver: prompts on disk, every inference knob exposed,
auto-rerun on save, error states that tell you what to do, ships on Windows +
Linux, keyboard-first coverage. Workspaces ship first so params, history, and
auto-rerun all attach to a real "current prompt file" from day one.

| # | Step | Status |
| --- | --- | --- |
| 2.4 | Workspaces & files | done (pending live GUI check) |
| 2.1 | Parameter controls | done (pending live GUI check) |
| 2.2 | Auto-rerun on save | done (pending live GUI check) |
| 2.3 | Prompt history | done (pending live GUI check) |
| 2.5 | Better error states | done (pending live GUI check) |
| 2.7 | Light + system theme | done (dark values need visual tuning) |
| 2.6 | Onboarding | done (pending live GUI check) |
| 2.10 | Keyboard shortcuts | done (pending live GUI check) |
| 2.8 | Windows + Linux builds | done (CI authored; needs a tag run) |
| 2.9 | Auto-update polish | done (pending live GUI check) |

Locked decisions: workspaces are folders of `*.quantamind.yaml` files (no
tauri-plugin-sql); hotkeys are a hand-rolled `useHotkey` (no new dep); Windows
ships unsigned in 2.8 (cert deferred to [Future considerations](#future-considerations)).
Never add in Phase 2: browser-based inference, team features, cloud sync
(multi-model comparison already shipped in v0.1).

Step-level acceptance gate (each step "done" only when all true): code merged
behind a `phase-2/<step>` PR; full Vitest + cargo suites green; output verified
per [Data quality](#data-quality); relevant docs updated in the same commit;
files single-concern (≤10 per folder); locked stack honored.

**Phase 2.4 complete** — backend 95 lib + 2 lifecycle integration; frontend 361
(11 new for workspaces). Verified via `workspace_lifecycle.rs` (round-trip, tree
order, rename/delete, hidden-dir skip, human-readable YAML). Not yet exercised in
a live Tauri window (folder picker needs a GUI + display).

### Phase 3 — v0.3 "The Bench" (complete)

A dedicated model-comparison Bench: per-panel params, output diff, metrics, saved
configs, and prompt templates.

Steps 3.1–3.3 done: the `InferenceBackend` trait, the llama.cpp `llama-server`
sidecar backend, and a left **backend switcher** that scopes the Workspace to
Ollama or llama.cpp. Steps 3.4–3.9 done: a dedicated **Bench** tab with
seq/parallel + VRAM gate, word-level diff, metric bar charts, saved
`*.bench.yaml` configs, a quantamind.co report footer, and a bundled
prompt-template library.

**Step 3.10 (cloud baseline) is dropped from Phase 3.** The
`InferenceBackend`/`BackendKind` design stays open to a cloud variant (see the
forward-looking notes in `inference/backend/`), so a future phase can add it
without rework. With 3.10 out of scope, Phase 3 ships at 3.1–3.9 and is complete.

### Phase 4 — v0.4 "Latency" (complete)

Performance instrumentation: surface the raw timing/memory signal behind a run
so users can see *why* it was fast or slow. Built one step at a time.

- **4.1 Per-token timing instrumentation (done).** `RunTiming` records a
  `TokenTiming { text, t_ms, n }` per accepted token (`t_ms` = ms since run
  start, monotonic; `n` = 1-based count). The `prompt-done` event's
  `DonePayload` gains a `timeline` array, mirrored by the frontend zod schema.
  The first entry's `t_ms` equals `ttft_ms` by construction. Foundation for the
  views below; no UI yet.
- **4.2 Token-timeline chart (done).** New **Latency** tab plots the last
  run's per-token latency (x = token index, y = gap from previous token) with
  the TTFT bar annotated and latency-spike gaps flagged as outliers (robust
  median/MAD modified-z rule). Pure math in
  `features/inspector/format/timeline.ts`; rendered with `visx` v4 (`@next`).
  Driven off `compareStore.rows`, so it shows **one labeled chart per model**
  for both single and multi-model runs (Compare now carries a per-token
  `timeline` on its done event). Hovering a bar shows a `#index · ms — token`
  readout.
- **4.3 TTFT breakdown (done).** `generate` now returns a `GenerateStats` from
  each backend's final chunk (Ollama's `load_duration`/`prompt_eval_*`/`eval_*`,
  ns→ms; llama.cpp's `timings`), carried on the done payloads. The Latency tab
  shows a stacked TTFT bar per model — Model load + Prompt prefill +
  Network/first-token (remainder) — segmented only by what the backend reports;
  otherwise "not available". Pure math in `features/inspector/format/ttft.ts`.
- **4.4 VRAM allocation (done).** `get_loaded_models` reads Ollama `/api/ps`;
  the Latency tab shows a per-model bar — In VRAM (`size_vram`) vs offloaded to
  system RAM (`size − size_vram`), `context_length` in the tooltip. Models not
  loaded / non-Ollama backends → "Not available" (never a fabricated
  weights/KV/free split). Device free/total VRAM deferred to 4.5 (GPU probe).
- **4.5 Hardware detection (done).** CPU/cores/RAM/OS/arch via `sysinfo` + a
  shell-out GPU probe (`nvidia-smi` CSV → bytes; macOS `sysctl` chip name +
  unified memory; else "Not available") in a new **Settings › Hardware** view.
- **4.6 Inter-token latency histogram (done).** Per-model visx histogram of the
  inter-token gaps; outlier bins highlighted (`format/histogram.ts`).
- **4.7 Cold- vs warm-start (done).** `HistoryEntry` records ttft/tok-s/load_ms;
  the Latency tab compares cold (load_ms>500ms) vs warm runs per model and shows
  the cold-load TTFT delta (`format/coldwarm.ts`).
- **4.8 Memory-leak heuristic (done).** `get_ollama_rss` (sysinfo processes)
  sampled per run into a session series; a banner flags a monotonic climb across
  5 runs (`format/leak.ts`).
- **4.9 Regression alerts (done).** Per model, the latest run is compared to the
  rolling 7-day average of prior same-prompt runs (from history); a ≥20% tok/s
  drop is flagged (`format/regression.ts`).
- **4.10 HTML report (done).** A self-contained inline-SVG/HTML report (hardware,
  per-model metrics, TTFT/VRAM bars, timeline + histogram, cold/warm +
  regression) exported via the dialog + `save_compare_report` (now allows
  `html`). Builders in `features/inspector/report/`.

**Phase 4 (v0.4 Latency) is complete (4.1–4.10).** Metrics stay nullable
(`null` = not measured, never `0`); reports follow
[#analysis-schema](reference.md#analysis-schema). Tests: full `cargo test`
green + `cargo clippy` error-free; 560 frontend (vitest) + `tsc`; `pnpm build`
succeeds. Shipped on branch `phase-4/per-token-timing`. Live GUI verification
pending (same gate as earlier phases).

### Phase 5 — v0.5 Quantization & Backends (complete)

Broaden *which* models and backends users can run, and help them pick the right
one. Built one step at a time.

- **5.1 MLX inference backend (done; live-verified).** A `MlxBackend` streams from
  `mlx_lm.server`'s OpenAI-compatible `/v1/chat/completions` (SSE), reached over
  HTTP — no FFI, consistent with the locked stack. **Apple Silicon only.**
  mlx_lm is user-installed (`pip install mlx-lm`), not bundled; QuantaMind only
  health-probes it read-only via `GET /v1/models` and shows MLX in the workspace
  backend rail only on Apple Silicon. `BackendKind::Mlx` listens on `:8082`
  (mlx_lm defaults to `:8080`, which collides with llama-server — launch with
  `--port 8082`). Wire mapping: `num_predict→max_tokens`, `top_k→top_k`,
  `repeat_penalty→repetition_penalty`; **`seed` is dropped** (mlx_lm has no seed
  field, so MLX runs are **not seed-reproducible**). Stats are token counts only
  (all `*_ms` stay `None` — mlx_lm reports no per-phase timing); TTFT and
  tokens/sec come from the client-side `RunTiming`. Sub-steps: 5.1.1 enum +
  endpoint + dispatch; 5.1.2 wire request; 5.1.3 stats; 5.1.4 stream + backend;
  5.1.5 cancellation; 5.1.6 health command + Apple-Silicon gate; 5.1.7 frontend
  rail + gating + not-detected hint; 5.1.8 docs; 5.1.9 model discovery — the
  loaded model is listed via `GET /v1/models` (a third source alongside Ollama
  and llama.cpp) so MLX is selectable and runnable; size/quant aren't reported
  by that endpoint, so they show blank rather than a fabricated `0`.
- **5.2 Model fit + MLX launcher (done).** Two parts. **5.2A:** hardware-fit
  badges on the HF download table — green "Fits" / amber "Tight" / red "Won't
  fit" per variant from `features/models/fit.ts` (the compare feature's
  1.3×-safety, 70%-tight rule); the column is omitted, never guessed, when no
  hardware snapshot. **5.2B:** QuantaMind now **starts `mlx_lm.server`** for a
  user-chosen HF repo (the dropdown-driven flow), reversing 5.1's "no in-app
  start" — `mlx_lm.server --model <repo>` downloads the repo on launch, so this
  is download + run in one flow. It mirrors the llama-server lifecycle with
  three hardenings: (1) **no false-fail** — start returns immediately, a stderr
  reader thread reports `Downloading`/`Starting`, readiness is the health probe
  (never a timeout during a multi-minute download), and `mlx_server_status`
  surfaces a died process's stderr tail; (2) **exit-reap** —
  `RunEvent::ExitRequested` kills the child (also llama-server) so no zombie
  holds memory/port; (3) **dynamic port** — `find_available_port(8082..=8092)`
  picks a free port stored in a process-global, and the MLX endpoint is
  state-derived (`mlx_endpoint()`), so health/discovery/dispatch follow it (no
  hardcoded `:8082`). Set `QUANTAMIND_MLX_SERVER` to override the executable
  path. (AWQ variants + resumable multi-file pulls deferred.)
- **5.3 Quantization comparison (done).** On the **Quant** tab, a chosen model's
  installed quants compare side-by-side: **size** + hardware **fit** (static),
  and **quality** = the 5.4 eval suite run per variant → a per-quant pass-rate
  (a variant whose backend errors is marked "error", never a misleading 0). A
  **"Compare speed in Bench →"** button loads the variants into the compare
  store and jumps to the Bench for **speed/TTFT/VRAM** (reusing the existing
  backend-aware compare runner from 5.6 rather than rebuilding metrics).
- **5.4 Built-in mini-eval suite (done).** Bundled `docs/evals/*.yaml` tasks
  (classification, reasoning, extraction, schema) run against any installed
  model from the **Tests** tab → a pass-rate + per-task pass/fail. **Scoring is
  deterministic** (locked stack has no sandbox/judge): exact-match,
  multiple-choice (first whole-word choice token), and — for the "code"
  category — **JSON schema-conformance** (BFCL-style): a balanced-brace
  extractor finds the first object that parses, then a flat depth-1 check of
  required keys + top-level types. Honest framing: a quality *smoke test*, not a
  rigorous benchmark. `inference/eval` (pure scoring, Tauri-free) + `commands/eval`
  (`list_evals`, `run_eval_task` — runs temp-0, accumulates output, scores).
- **5.5 Smart quant recommender (done).** A **Quant** view — reached via the
  **Analysis → Quant** sub-tab: pick a model that has
  several installed quantizations (grouped by family + size in `quantPick.ts`)
  and a use case (fast-chat / quality-writing / coding / reasoning) → a
  recommended quant + plain-language why. Pure `recommendQuant` combines the
  `HardwareSnapshot` fit (reusing `memoryFit`'s 1.3× rule) with a quant-quality
  rank: fast-chat picks the smallest fitting quant, quality use-cases the
  highest-quality one; honest when nothing fits or hardware is unknown.
- **5.6 Backend auto-selection (done).** A backend is **coupled to the model's
  weight format** — an MLX model runs only on MLX, a GGUF only on
  llama.cpp/Ollama — so selection is the absolute `model.backend` mapping, never
  a health-based fallback. Compare rows are now **backend-aware** (`rows_for`
  takes a backend per model; `run_compare` forwards each model's backend, so a
  mixed-backend compare dispatches each row to the right server — previously all
  rows wrongly went to Ollama). When the required backend isn't healthy, Run is
  **blocked with a hint** ("Start the MLX backend to run this model") rather than
  rerouting (`features/workspace/state/runHint.ts`).
- **5.7 Model card viewer (done).** Real READMEs are arbitrary HTML, so the card
  is treated as a **data source, not a document**: `hf_model_card` fetches the
  README and `to_card` reduces it to structured data — `license`, `base_model`,
  `pipeline_tag`, `tags` (parsed from the YAML frontmatter via the existing
  `serde_yaml`, handling string-or-list shapes) + a `description` (the first ~3
  prose paragraphs, skipping HTML/tables/headings). The frontend
  `ModelCardDetail` maps that JSON to **native components** — badges, a tag list,
  the description, and an **"Open full card on Hugging Face →"** button (shell).
  Crash-proof by construction (controlled values, never injected HTML); 404 →
  "no model card", not an error.
- **5.8 Tool-calling reliability eval (done).** The agentic sibling of 5.4: which
  model/quant/backend can drive an agent, measured **offline, deterministically,
  judge-free**. **Prompt-based** (tool schemas in the system message, parse the
  JSON call from the completion — backend-agnostic), **single-turn**, **greedy
  (temp 0)**. Scoring is **BFCL-style structural** (name + structural args, not
  execution) over a curated ~13-task fixture (single / select / parallel /
  abstain), labelled *indicative, prompt-based, structural*. Two metrics kept
  separate via **cascaded conditional denominators** so a format error doesn't
  bleed into reasoning: `parse_rate` (over call-expected tasks), `tool_selection`,
  `args`, `abstain` — each `Option` (n/a, not 0, on a 0 denominator); composite =
  mean of available. The greedy extractor handles arrays AND bare sequential
  objects; parallel scoring is length-guarded 1:1. Surfaced on the **Tests** tab
  (`ToolCallPanel`) and as a per-quant spread in the **Quant** view.
  `inference/eval/toolcall/` (Tauri-free). The per-quant spread is a one-line
  headline in the **Quant** view (e.g. `Q4_K_M 71% · Q8_0 88%`).

- **5.9 Custom-eval manager (done).** The suite is dynamic: run the curated
  built-in set **or** your own authored collections. The runner is
  **storage-decoupled** — `run_toolcall_eval(model, backend, tasks)` is always
  handed a `Vec<ToolTask>` (built-in via `get_builtin_tasks`, or a loaded
  collection) and never touches files. Collections are one **`.json` per file**
  under `app_config_dir/evals/` (portable, VCS-friendly), with Rust-owned CRUD +
  a **path-only import** (`import_custom_collection(source_path)` reads, size-caps
  at 1 MiB, and validates — the frontend never reads file bytes across IPC).
  **`validate_tasks` is the single backend-side trust boundary** for *any* task
  source (built-in, saved, imported, hand-edited): non-empty tools, known
  category, a JSON-Schema `parameters` block (validated via a strict serde
  struct), category⇔`expected` coherence, and calls only to offered tools — bad
  input → `AppError::InvalidTaskSchema` naming the field. Authoring is a plain
  textarea + **Insert Example** (one task per shape) + **Check JSON** (Zod, UX
  only). `tools[].parameters` adopts the nested **JSON-Schema** shape developers
  paste from real tool defs. `commands/eval/eval_registry.rs` +
  `persistence/evals.rs`; UI in `features/eval/` (`DatasetBar`, `EvalEditor`,
  `useEvalRegistryStore`).

- **5.10 Model inspector + template/base-model guard (done).** Local diagnostics
  from Ollama's `/api/show`: the chat **template** (rendered as inert text, never
  injected HTML), the reported **capabilities**, and an **advisory base-model
  guess** — flagged when the template has no chat-role markers AND `tools` isn't a
  capability, with the **evidence** surfaced (`base_reason`) so it reads "likely
  base — …", not an absolute claim. **Ollama-only** (the data lives in `/api/show`);
  other backends show "Not available — Ollama only". `inference/ollama/ollama_show.rs`
  (Tauri-free client; raw `model_info` kept for the 5.11 KV predictor) +
  `commands/models/model_inspect.rs`; UI `features/models/.../TemplatePanel.tsx` on
  the Tests tab. First of the **5.10+ diagnostics** band (metadata + local math).

- **5.11 KV-cache VRAM predictor + bandwidth (done).** Predicts VRAM as **base weights + KV
  cache(context)** so users stop guessing whether a model+context fits. The canonical f16 KV
  formula lives in `inference/vram_math.rs` (`2·layers·kv_heads·head_dim·2·ctx`, unit-tested:
  Llama-3-8B @ 8k = 1 GiB) and is exposed via `estimate_kv_cache_bytes`; the Quant tab's
  **context-length selector** (4K/8K/32K/128K, capped at the model max) drives it. Dims come from
  Ollama `/api/show` `model_info` (on `ModelInspect.dims`); non-Ollama falls back to the file-size
  ×1.3 heuristic, flagged **approximate**. A quant whose `base + KV` exceeds available memory gets an
  **"OOM Risk"** badge and is **blocked from running** (only when hardware is known); the
  recommendation respects the same gate. A note states local-LLM speed is **memory-bandwidth-bound**,
  showing the curated GB/s or "Not available". `vram_math.rs` + `hardware_mem.rs` (bandwidth) +
  `features/quant` (`useVramFit`, `QuantPage`, `fit.ts::fitOfNeed`).

- **5.12 Silent-CPU-fallback guard (done).** The Tests tab warns when the selected model is loaded
  with weights off the accelerator (the silent fallback that tanks speed and ruins eval timings):
  `cpuOffload(size, vram)` over `/api/ps` data, gated on an accelerator being present. Ollama-only;
  renders nothing when fully resident / not loaded / other backend. `features/eval/CpuFallbackBanner`.

- **5.13 Quant parse-rate delta (done).** The Quant table shows the quality lost to a smaller quant
  in percentage points vs the highest-quality scored quant (e.g. Q4_K_M "−17pp") + a "Δ vs Q8_0"
  note on the spread line. Pure `toolcallDelta`.

- **5.14 Context-budget readout + finance preset (done).** (a) A `ContextBudgetBar` in the Latency tab
  shows the exact `prompt_eval_count / context_length`, red ≥95% (about to overflow and drop tokens);
  "Not available" when unknown. (b) A second **read-only built-in preset**, "Finance (preset)"
  (`tasks_finance.json`), via `list_builtin_collections` / `get_builtin_collection(id)`; the dataset
  picker lists Curated + Finance as read-only. Structural tool-call reliability — **not** PDF parsing.

- **5.15 Context Stress Test (done).** Runs the selected dataset at growing prompt lengths and graphs
  where tool-call accuracy collapses (`cliff.ts` + `useContextCliff` + a visx `ContextCliffChart` on
  the Tests tab). **Frontend-only**, padding is approximate (≈tokens via chars/4) and **labelled
  indicative** — no tokenizer; a failed rung records null, never a fabricated score.

### Phase 6 — Agentic reliability & the Automated-Pipeline workspace

Turns the single-turn checker into a stateful agentic diagnostic engine AND
reshapes the eval experience into a two-zone app. See [Agentic reliability
eval](reference.md#agentic-eval) for the contract.

**Navigation: two zones.** Zone 1 (Manual Playground): Workspace · **Compare**
(renamed from Analysis) · Latency. Zone 2 (Automated Pipeline): **Tests** (the
3-pane workspace) · **Audit** (compliance home).

| # | Step | Status |
| --- | --- | --- |
| 6.1–6.4, 6.6 | Agentic engine: `DeterministicSandbox`, Tauri-free `run_once`/Pass^k runner, `FailureTracker`, effort metric, anti-cheat | done |
| 0 | Nav: Analysis → Compare, new Audit tab | done |
| 1 | Data contract: `ToolTask.agentic`, `EndStateRule` enum (`RequireSequence`/`ExpectAbstainingText`), `validate_tasks` gate | done |
| 2 | VRAM-safe sequential `run_batch` dispatcher + `run_batch_eval` streaming command | done |
| 3 | Throttled single-stream React consumer (rAF-buffered `batchStore`, `useBatchRun`) | done |
| 4 | 3-pane Tests workspace: `MatrixScoreboard` + `TraceDebugger` + audit export | done |
| 5 | Audit tab: Results History + export + Context Stress Test | done |
| 6 | Driver B lazy-agent traps: per-call `FaultInjection` (transient/persistent) in the sandbox + runner (`ToolError` step), authored in the Configurator | done |
| 7 | Driver D schema resilience: semantic `validate_call` + `max_recovery` loop (`SchemaError` step, `MalformedSchema` failure, `schema_resilience` metric) | done |
| 8 | Model Results `Schema Resil.` + `Context Limit` columns (context limit shared with the Latency budget gauge via the `quantamind-cliff-<model>` marker) | done |
| 9 | CSV import: strict flat `id,prompt,expected_tool,expected_args` + shared Tools box → custom collection (`read_text_capped`, `csvImport.ts`, `CsvImportModal`); single-turn only, live located validation | done |

Locked decisions (set during planning): **iterate in Rust, never React** — one
`run_batch_eval` command runs a strict sequential model×task queue and streams
back over a single channel (no N-command JS loop → no local-inference OOM); the
runner is generic over a `ModelTurn` seam (testable with a scripted model, no
HTTP); `EndStateRule` is an enum so a correct abstention isn't scored as a lazy
failure; the React consumer buffers events and flushes at ≤60Hz so a token
firehose can't freeze the UI; the **Matrix is the central Eval artifact** (per-
model rows), not buried.

Follow-ups: persist each `run_batch_eval` to history (extend `RunSummary` with
agentic metrics) so the Audit timeline reflects batch runs; retire the now-
orphaned `run_collection_matrix`/`run_toolcall_eval` commands + `MatrixPanel`/
`ToolCallPanel` components once nothing references them.

### Phase 7 — Local Agent Readiness Validator

Packages the Phase-6 engine into a transparent **"is this model ready to replace
my cloud agent"** verdict. See [Local Agent Readiness](reference.md#agent-readiness)
for the contract.

| # | Step | Status |
| --- | --- | --- |
| 7.1 | Pure `readiness::assess` verdict model (Ready/Conditional/NotReady) — epsilon-guarded thresholds, strict null-gating (required-but-unmeasured blocks), one scoring source of truth for GUI + future CLI | done |
| 7.1 | Editable built-in profiles (Coding agent / RAG assistant / General agent) as flat JSON via `persistence/readiness/profiles` + collision-proof `safe_filename` | done |
| — | `AggAgentic` carries the full `FailureTracker` so the loop/hallucination gates see exact counts, not just `top_error` | done |
| — | Persist the last `BatchReport` per collection (`persistence/readiness/reports`) — Rust is the verdict's source of truth, not the frontend store | done |
| 7.7 | **Agent Report** tab: pick a collection + profile, run `assess_readiness`, render per-model badges + interpolated reasons + measured path; shareable offline HTML export | done |
| 7.4 | **Hardware Telemetry**: measure VRAM fit (exact weights + real KV cache at the run's `num_ctx` vs an allocation cap) via the pure `readiness::vram_fit`; Host Hardware Profile panel (arch + cap dropdown) + per-model memory line; flips `require_full_vram` on for Coding-agent | done |
| 7.2 | **Native-FC test mode**: run the same agentic tasks through Ollama's native `/api/chat` `tool_calls` API (`NativeOllamaTurn` translates back to the canonical call shape, so the sandbox/scoring are byte-identical); parallel **Native FC pass^k** Matrix column behind a toggle; the verdict **prefers native** when measured. Ollama-only — llama.cpp/MLX N/A | done |
| 7.5 | **Resumable job queue + VRAM isolation**: append-only `.jsonl` job log per run (`persistence/jobs`), healed on a truncated tail; `run_batch_resumable` skips completed units (prompt AND native) and appends each new one; an injected `VramGate` (`OllamaVramGate` = `keep_alive:0` + `/api/ps` poll, **assert-and-fail**) evicts the previous model before the next loads; resume bulk-paints the Matrix from one partial report then streams the tail; transactional finish (save → verify → delete log) | done |
| 7.3 | **Agentic-aware recommender**: `assess_readiness` returns verdicts **ranked best-first** via the pure `readiness::recommend::rank` (tier Ready>Conditional>NotReady, ties by effort then steps, native-first metrics, float-safe `total_cmp`/`None→MAX`); the Agent Report opens with a leaderboard + a Recommendation banner ("Recommended for {profile}: {model} (Ready)", or "no model is ready — closest" — never a fabricated Ready) | done |
| 7.6 | Headless `quantamind-cli` | **dropped** |

**Phase 7 complete.** Deferred follow-ups: llama.cpp native-FC; the `unmeasured`/🔧
badge (lands with a real probe to trigger); per-run (k-level) job granularity.

Locked decisions: **never fabricate** — an unmeasured hard-required metric blocks
(ignorance is not a pass), unknowns render N/A, prompt-based vs native paths are
labelled never conflated; thresholds live in **editable profiles**, not constants;
built-in profiles gate only on metrics measured today (Pass^k, loop/hallucination
taxonomy, steps, and — since 7.4 — VRAM fit on Coding-agent), so a default profile
doesn't mark every model NotReady for infra reasons. The **Context Stress Test is now
wired** end-to-end (Model Results pre-fills the probe → measured context limit saved per
(collection, model) → fed into the verdict → shown in the Agent Report), but the
`min_context_tokens` gate stays **opt-in** (off in the built-ins; a custom profile
turns it on) so an un-probed model is never silently failed. VRAM fit is **Ollama-precise**
(real `/api/show` dims) — single-model backends are N/A, never approximated; the
cap is auto-detected and overridable in-session (not persisted). The verdict scoring
is **one function** (`assess`) so GUI and the future CLI can never diverge.

### Phase 8 — Publish & Community (OSS client, PART B)

Turns a local readiness verdict into a shareable asset and (opt-in) aggregate data
for the recommender. Phase 8 spans two systems: a **closed backend** (separate
private, hosted repo — token authority, publish API, validation, dedup, leaderboard,
baselines, moderation; out of scope for this open repo) and the **OSS client** flow
here. The app stays 100% functional offline — a publish/auth failure never touches it.

| # | Step | Status |
| --- | --- | --- |
| 8.B4 | **Offline share export** — fully offline, no auth, no backend. Pure `buildReadinessMarkdown` (GFM table + per-model reasons, "N/A" never fabricated); `snapshotPng` rasterizes the report card via `html-to-image` (embedded fonts + warm-up render; hardcoded white background); a thin `save_readiness_image` Rust sink writes the PNG bytes (path via OS dialog). The Agent Report's **Export Report** menu offers Image / Copy Markdown / HTML | done |
| 8.B2 | Canonical record (`persistence/publish/`, pure: metrics-only `PublishRow`, sorted-key deterministic JSON + SHA-256 hash + local `pre_validate`) + `commands/publish/cohort.rs` (v1 hardware cohort key) + `preview_publish_payload` + privacy gate (`PublishDialog`: shows the exact raw payload, default opt-out) | done |
| 8.B1 | PKCE auth + hybrid vault (`commands/publish/{auth,pkce,token,login_cmd}.rs`, `keyring` with in-memory fallback when no secret service; S256 challenge; loopback redirect; rotating refresh + cached access token) — `enterprise`-gated, verified against `mockito` | done |
| 8.B3 | Send pipeline (`publish_cmd.rs`: fresh nonce → canonical hash → batch POST; typed `PublishOutcome` for 200/401/422-with-index/426/429, **fresh nonce per attempt**) + walled-garden write-up link + UI wiring (`PublishButton`, never freezes) — `enterprise`-gated, verified against `mockito` | done |

Locked decisions: **server-side everything** (the open client is untrusted; the
backend enforces rate limits, validation, dedup, anti-replay); **no client secret**
(PKCE); **privacy gate** sends metrics + cohort tags + signature only, never task
content/prompts/file names, default opt-out, disabled in enterprise builds; results
are labelled **"community-reported"** (self-fabrication is deterred via validation +
outliers + GitHub identity + report/remove, never cryptographically prevented). The
**export (B4) is ungated** — it ships in every build, including enterprise.

### Phase 9 — Difficulty tiers & anti-saturation

Graduated difficulty for the agentic eval: per-task **tiers** (Easy→Extreme) that
scale Pass^k, **hardware-calibrated** readiness (the bar rises with the machine), a
**v2 scenario engine** (19 bundled tiered collections replace the hand-coded
fixtures), seeded **decoy** injection, and **procedural instancing** for
contamination resistance. The engine internals are documented in
[reference.md](reference.md) (difficulty tiers + the v2 scenario engine); the run-UI
surface is under [reference.md#eval-runner](reference.md#eval-runner).

| # | Step | Status |
| --- | --- | --- |
| 9A | `Tier` + `DifficultyAxes` on `AgenticSpec` (serde-default, byte-identical round-trip); seeded decoy injection (`agentic/difficulty/decoys.rs`); `pass_k_for(tier)` (Easy 5 / Medium 8 / Hard 16 / Extreme 24, explicit `k` wins); `unknown_tool_calls` distraction tally (excluded from `top_error`); trivial-agent + decoy-isolation gate tests | done |
| 9B | Hardware-calibrated tier gate: `hwclass.rs` (memory → `HardwareClass` → `default_required_tier`); `AggAgentic.by_tier` strict Pass^k per tier; `ReadinessProfile.required_tier` + `assess()` blocks only when the required tier was **exercised but not cleared** (untested = NotAttempted, never a guessed fail); verdict carries `required_tier`/`cleared_tier`; Agent Report renders graduated readiness per row | done |
| v2 | Full v2 scenario engine (`agentic/v2/`): `world_state` discovery, `RequireAll` unordered checkpoints, `must_not_call` traps, name-keyed faults; 19 bundled tiered collections become THE eval content (old fixtures deleted); runs on the unchanged agentic runner; integrity + oracle gates prove all 434 tasks satisfiable and a trivial agent scores 0 | done |
| C1–C2 | Per-run sandbox factory (`run_agentic_with`) + procedural instancing (`v2/generator.rs::instantiate`): seeded bijective entity-id rename per Pass^k run (oracle-safe, novel per run, replay fallback when no numbered entities); `AgenticSpec.generated` threaded through both streaming + native passes | done |
| A5b/B2 | Runtime safety for heavy v2 runs: per-step 180s turn timeout (`TurnTimeout`); mid-run cancellation checked between Pass^k runs; worst-case pre-run cost estimate beside the Matrix Run button | done |
| 9-UI | **Inline tier + anti-saturation run controls** on the Tests page: Difficulty Tier dropdown (`Auto`/Easy…Extreme/`Custom`) with tier-locked Pass^k (`PASS_K_BY_TIER` mirrors `passk.rs`), `get_hardware_tier` command driving `Auto` + the HW hint, an Enable-Decoy-Tools budget, and scoreboard header chips (`Tier · K · Decoys`). `tier`/`decoyTools` flow through `run_batch_eval` → `apply_overrides` onto each agentic spec | done |
| 9B-report | **Agent Report deep-dive** (per-model, augmenting the multi-model table): per-tier `avg_steps` + `failures` now computed in `agg_agentic` (enriched `TierStat`) and exposed on `ModelVerdict.by_tier`/`failures` via one `native_first_source` helper (same source the gate reads). Three new components — **Executive Verdict** (run-tier headline = highest tier exercised; hardware class is an advisory lens, never a gate; status = "cleared what it ran" via a contiguous `clearsThrough`), **Tier Progression Matrix** (CLEAR/SATURATED/FAIL/NOT-TESTED on the same `min_pass_k` bar as `cleared_tier`; Task Parameters from real task axes or "not declared"), **Failure Taxonomy** (failure-mode distribution across the *tested* tiers, honest event denominator). Versioned JSON export | done |
| 9-publish | **Phase-8 publish payload extended with the Phase-9 verdict** (`backend-publish.md`): `PublishRow` (allowlist) now carries `status`/`eval_method`/`tier_tested`/`cleared_tier`/`hardware_class`/`recommended_tier`, the per-tier curve (`by_tier` = rate/k/decoy), `failure_distribution` (counts mapped field-by-field from `FailureTracker`, incl. `reported_in_prose`), collection identity (`collection_name` + SHA-256 `collection_hash`), and provenance (`schema_version`/`engine_version`/`build_hash` from `build.rs`). `project` gains a **built-in-only gate** (custom collections excluded via `collection_hash: None`); the canonical hash covers the full row. Integrity stays **hash-only** (no client signature). | done |

Locked decisions: an authored per-task `k` **no longer silently wins** under a chosen
tier (the backend stamps the derived Pass^k so the locked display matches the run);
`Custom` is the escape hatch that restores free-`k`, no-tier behavior. **Max Steps
stays editable** — there is no tier→max-steps policy to lock it to. The GB→class→tier
thresholds live **once** in `hwclass.rs` (read via `get_hardware_tier`), never
duplicated in TS. Deferred: deep per-template *semantic* generation (instancing does
generic id-remap only), replay-vs-varied status per collection, tier-tagging of
imported custom collections, the D-series leaderboard (needs Phase 8), and a
**per-collection opt-in** to publish results on custom collections (today they are
hard-excluded from the leaderboard payload).

### Phase 9+

Owners flesh out the next phase's section here when the current lands.

---

## Future considerations

Parking lot for ideas, libraries, and changes deliberately deferred. Nothing
here is in the current phase — see [Phase roadmap](#phase-roadmap). If something
here becomes relevant, move it into a phase plan first.

### Cargo workspace split (per-layer crates)

The backend is a single crate with layers as modules — the stage
[`../rust-engineering-architecture-guide.md`](../rust-engineering-architecture-guide.md)
Part 3 endorses until scale forces a split (see
[`adr/0001-single-crate-not-workspace.md`](adr/0001-single-crate-not-workspace.md)). The
guide's Part 3c target is a Cargo workspace where the *compiler* enforces the Dependency
Rule instead of a guard test: `engine-core` (domain — `inference/` minus the HTTP
clients), `engine-app` (use cases), `backends` (Ollama/llama.cpp/MLX driven adapters),
`commands` (the Tauri driving adapter), and a `bin/` composition root.

**Activate when** one of these triggers fires (not before):
- clean-build time crosses ~a few minutes and incremental builds hurt the loop;
- more than a couple of engineers work the backend concurrently and the guard-test
  enforcement (vs compiler enforcement) starts letting cross-layer leaks through review;
- a layer needs to ship independently (e.g. a headless CLI reusing `engine-core`).

**Why deferred:** it touches all ~353 files and the Tauri build, for a boundary the
`layering_guard.rs` test already enforces at current scale. The litmus test
(`inference/` compiles with infra deleted) already holds (Phase 3). Do it as its own PR,
one crate extracted per commit, suite green between each.

### Stricter lint gate + richer test stack (locked-stack PRs)

These align with the guide (Parts 6–7) but each adds a crate or a cleanup the **locked
stack** (`#tech-stack`) gates behind a rationale PR — so they are proposals, not yet code
(see [`adr/0006-test-libraries-deferred.md`](adr/0006-test-libraries-deferred.md)):

- **`proptest`** — property tests for the determinism-critical seams: `pass_k` bounds,
  scoring bijection, and `serde` round-trips of specs (a round-trip property catches the
  "schema silently drops an unlisted field" bug deterministically). Highest-value add.
- **`insta`** — snapshot tests for complex *deterministic* output (a readiness report, a
  serialized env view). Snapshot the deterministic projection, never stochastic model text.
- **`cargo clippy -- -D warnings`** — promote the CI clippy step from an error gate to a
  warning gate **after** the ~49 pre-existing warnings are cleared (a dedicated cleanup).
- **`cargo fmt --all -- --check`** — add once the tree is reformatted (`cargo fmt` once,
  one mechanical commit); today the code is not rustfmt-clean and the gate would red-wash.
- **Considered, not now:** `mockall` (hand-written in-memory fakes already cover the
  ports), `cargo-nextest` (suite runs in <1s), `async-trait` (backends use enum dispatch,
  no `dyn`). Revisit if the suite or the port surface grows.

### Full end-state agentic Context Stress Test

The Context Stress Test now *includes* agentic collections, but scores them on JSON **well-formedness**
only — does the model still emit a parseable tool call as context grows (see `reference.md#context-cliff`).
That catches a real failure mode (the tool-call FORMAT degrading with context) but **not** task
correctness. A fuller probe — "at what context length does **multi-step** agentic accuracy collapse?" —
would run the sandbox conversation under padding, decide where the needle lands among turns, and
aggregate **Pass^k per rung** against `agentic.end_state`. **Activate when:** a phase wants correctness
(not just format) context-headroom signal for agentic workloads. **Why deferred:** it's a new multi-turn
engine (padding placement across turns, per-rung Pass^k), not a tweak to the single-turn probe.

### Medium/Hard answer-key reachability

The v2 reachability guard (`scenarios.rs::every_required_easy_world_state_fact_is_tool_reachable`)
asserts that a world_state fact a checkpoint forces the model to echo is retrievable through some
getter — but it is **enforced on the Easy tier only**. The **Extreme** tier is deliberately excluded
(its required values — chosen statistical method, etc. — are *reasoned conclusions*, not facts to
fetch), and **Medium/Hard are tracked here**. Diagnostic worklist of unreachable facts:
`medium-coding/md_co_trace_root_cause`→`payments/round.py`,
`medium-coding/md_co_rollback_by_severity`→`checkout_v2`,
`hard-coding/hd_co_ci_multifile`→`billing/format.py`+`billing/rounding.py`,
`hard-coding/hd_co_incident_forensics`→`key-reports-3`+`svc-reports`.

**The load-bearing principle (do NOT skip):** the Medium/Hard pass is **not** "make every flagged fact
reachable." It is **classify each required fact as RETRIEVAL vs DERIVATION, then enforce reachability
only on the retrieval ones.** Over-enforcing turns a reasoning challenge into a lookup (the Extreme
mistake, one tier down); under-enforcing leaves the harness-fails-a-correct-model bug. The Extreme
exclusion is the worked example — carry it into this work.

**Why deferred (it's NOT another mechanical key-fix):** the Easy repairs were key-alignment (re-key
`cart_tests`→`cart`, flatten `symbol`) — mechanical. Medium/Hard needs the per-fact retrieval/derivation
judgment AND **re-validating that the procedurally-generated instances still produce the intended
difficulty** after the world_state restructure (these tiers carry `generator`/`instance0` + entity
remap; reshaping world_state can perturb instantiation). That generated-instance re-validation is the
real cost. **Activate when:** a phase invests in Medium/Hard answer-key correctness. **Mechanics:**
restructure world_state so retrieval facts resolve through a called getter, then widen the reachability
test to those tiers (drop the Easy-only `continue`) and re-run BOTH the oracle test and the
generated-instance satisfiability checks to confirm gradeability + preserved challenge.

### Retrieval-vs-derivation over-enforcement is already live on Easy (`branch_target`)

The over-enforcement the section above frames as a Medium/Hard *future* risk is **already biting on
Easy**. `easy-coding/es_co_branch_target` lists `get_change{id:C-1}` and `get_change{id:C-2}` as
required `RequireAll` checkpoints — but the prompt itself **states** "C-1 is a hotfix, C-2 a feature."
The change kind is *given*, so a model that correctly derives the base branch (hotfix→`release`,
feature→`develop`) and opens both PRs **without** calling `get_change` is doing the task's actual
skill, yet fails as `HallucinatedCompletion` (it satisfied 2/4 — only the `open_pr` calls). Confirmed
**not** an echo/ack bug: the action-ack composes correctly (`runner_tests.rs::worldstate_multi_call_actions_each_ack_not_echo_entity`),
so the failure reason is purely "skipped the discovery calls." The consequence is the founding
inversion — the benchmark **understates** a capable model.

**The decision is mutually exclusive — you cannot have both:** (a) drop `get_change` from
`branch_target`'s required checkpoints (the task's skill is *routing by kind*, not *calling a getter*;
the kind is disclosed, so the lookup is ceremony), OR (b) remove the kind from the prompt so the
`get_change` lookup is genuinely required (the model must discover it). Leaking the answer in the
prompt **and** mandating the lookup is the contradiction. Lean **(a)** for `branch_target` — but it's a
per-task judgment, same as the Medium/Hard worklist above. **Action:** audit EVERY Easy task for the
same shape (prompt discloses a fact a checkpoint then forces a getter to re-fetch) and fold the hits
into the retrieval-vs-derivation worklist — Easy is no longer exempt.

### Grader-bug fixes landed (G1/G2/G3) + the abstain-pattern audit

The Qwen run (clean JSON, no format noise) surfaced grader bugs failing capable models. Fixed:
- **G1 (prompt↔grader contradiction):** the system prompt's closing line is now end_state-gated
  (`build_system_for(tools, TerminalGuidance)`) — act-tasks mandate a tool, abstain-tasks keep plain
  text. The 9 reporter tasks (terminal `reply`/`reply_customer`) no longer fail a correct prose answer.
- **G2 (ceremony getter):** dropped the redundant `read_file` checkpoint from `es_co_run_failing_test`
  (the failing-test name comes from `run_tests`; `read_file` could only ack a wildcard path) and
  softened its prompt. NO broad getter-resolvability guard — verified it false-fires on Easy derivation
  getters (`chem_lookup`/`convert_units`/`convert_temp`/`format_value` ack by design; the answer is
  *derived*, not retrieved).
- **G3 (honest label):** `reported_in_prose` failure mode — a no-call yield where every other
  checkpoint is satisfied and the prose matches the lone reporter checkpoint's text glob (the
  "exactly one unsatisfied" guard stops a weak glob like `*3*` relabeling a real hallucination).
- **G1 follow-up (reply-tool-aware mandate):** G1's first wording told every act-task to "use the
  `reply` tool" — but ~55 action-only tasks have none, so a model that couldn't complete tried a
  phantom `reply` call → schema-error→timeout. Fixed: `build_system_for` now names the REAL reporter
  via `reply_tool_name` (or, action-only, "your tool actions are your answer"). The all-collections
  invariant guard surfaced 2 tasks (`es_ms_sigfig_domain`, `es_ms_temp_scale`) carrying a **vestigial
  `reply` tool** the grader never required (terminal is `format_value`/`convert_temp`) — removed it so
  they're cleanly action-only.
- **`es_co_branch_target` get_change over-enforcement — RESOLVED (lean (a)).** Dropped the two
  `get_change` checkpoints (the prompt states the kinds; the skill is routing, not the lookup); now
  passes for a model that routes hotfix→release/feature→develop. `must_not_call` traps unchanged
  (guarded by `runner_tests.rs::branch_target_wrong_base_trap_stays_terminal_after_get_change_drop`).

**OPEN — abstain-pattern audit (follow-up).** `es_cs_abstain_no_tool` was a true mis-authoring
(named/intended as abstention but authored `RequireAll([reply_customer])`); fixed by converting it to
`ExpectAbstainingText`. A scan for the general shape (name/prompt implies abstention/no-action but
end_state is `RequireAll`) found other candidates that are **legitimate decline-VIA-action** — they
have a dedicated decline tool: `es_ec_price_match`→`decline_match`, `md_lg_data_request_by_law`→
`refuse`, `hd_se_flash_sale_oversell`→`offer_backorder`, `hd_fi_margin_call_cascade`→`log_decision`.
**Do NOT "fix" these** — a decline routed through a real action tool is correct RequireAll. Only a
name-implies-abstain task with NO decline tool (so the only way to "pass" is an unrelated reply) is a
mis-authoring like `abstain_no_tool` was.

### Additional STT engines (faster-whisper)

**Removed:** `mlx-audio` was trialed as a second STT engine but removed — its
0.4.4 server crashes during transcription (its inference broker runs the model on
a worker thread with no Metal GPU stream), and the transcription endpoint only
exists in that broken release. The MLX whisper model itself works *inline*, so a
future engine could wrap `mlx-whisper` directly (bypassing mlx-audio's server),
but that's a custom sidecar with its own maintenance cost — not worth it while
whisper.cpp covers Apple Silicon well. (The MLX **LLM** backend is unaffected.)

**Why deferred:** `faster-whisper` (the path for Ollama users, who have no native
STT) gets its own `commands/stt/<engine>` lifecycle. **Activate when:** a later
phase needs Ollama-user parity. It exposes an OpenAI-compatible endpoint, so it
must keep the loopback-only `stt_probe` guardrail (never silently reach
`api.openai.com`).

### Apple Developer ID + notarization (macOS)

**Why deferred:** $99/yr Apple Developer Program + ~24-48h approval. Current
ad-hoc signing (`signingIdentity: "-"`) eliminates the "damaged" error; testers
right-click → Open past the "unverified developer" warning. Acceptable for
v0.1.x tester distribution. **Activate when:** moving to a public one-click
download, or a tester refuses to right-click → Open.

Enrollment checklist: enroll at developer.apple.com/programs; in Xcode →
Settings → Accounts add the Apple ID, then Manage Certificates → "+" →
**Developer ID Application**; note the **Team ID**; generate an **app-specific
password** (label "notarytool"); export the env vars before `scripts/release.sh`:

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="abcd-efgh-ijkl-mnop"   # the app-specific password
export APPLE_TEAM_ID="TEAMID1234"
```

`scripts/notarize.sh` detects the env vars, re-signs the `.app`/`.dmg` with
hardened runtime, submits to Apple, waits, and staples the ticket. Verify with
`xcrun stapler validate dist/*.dmg` and `spctl --assess --verbose=4`. Known
notarytool rejections: missing secure timestamp (add `--timestamp`), JIT not
entitled (add `com.apple.security.cs.allow-jit` to `macos.entitlements`),
hardened runtime not enabled (confirm `--options runtime`).

### Windows code-signing certificate

**Why deferred:** an OV/EV cert is $200-400/yr plus identity verification; EV
certs ship on a hardware token (or cloud HSM) that CI must reach. v0.2 ships the
`.msi`/`.exe` **unsigned**, so SmartScreen shows "Windows protected your PC"
once — users click **More info → Run anyway**. **Activate when:** Windows becomes
a primary target or a tester refuses the warning. Checklist: buy OV (instant-ish)
or EV (clears reputation faster); for OV export the `.pfx`, store base64 as
`WINDOWS_CERTIFICATE` and password as `WINDOWS_CERTIFICATE_PASSWORD`; set
`bundle.windows.certificateThumbprint` in `tauri.conf.json`; EV needs a cloud-HSM
signing service. Verify with `signtool verify /pa`.

### Intel Mac build target

Current builds are `aarch64-apple-darwin` only; Intel users get a "platform not
found" from the updater. Add `x86_64-apple-darwin` + a universal-binary `lipo`
step when an actual Intel tester appears.


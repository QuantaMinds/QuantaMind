# QuantaMind CLI (`qm`) — reference

`qm` is the **headless** face of QuantaMind: the same agent-readiness engine as the desktop app,
driven from the terminal for first-run setup, CI gating, air-gapped runs, and scripting. It talks
directly to your local or remote inference server — no GUI, no phone-home. Every command prints
human text, or a machine-readable object with `--json`.

## Quickstart — three commands to your first verdict

Build once, then **connect and get a verdict the way `gh auth login` / `gcloud init` work**: one
diagnose command, one zero-config init. After the one-time build, connecting takes seconds — every
failure prints the exact fix command, so you're never stuck googling.

**0 · Prerequisites** (skip what you already have)

- **Rust 1.75+** — macOS `brew install rust` · Linux `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` · Windows `winget install Rustlang.Rustup`
- **A running inference server with ≥1 model.** Fastest path is Ollama:

```bash
# macOS: brew install ollama · Linux: curl -fsSL https://ollama.com/install.sh | sh
# Windows: winget install Ollama.Ollama (runs as a service after install)
ollama serve &
ollama pull qwen2.5:3b        # ~2 GB — a good first model to gate
```

Already running llama.cpp / MLX / vLLM / SGLang instead? Skip this — `qm doctor` finds whatever is up.

**1 · Build the binary** (one-time; the first compile takes a few minutes)

```bash
git clone https://github.com/QuantaMinds/QuantaMind.git
cd QuantaMind/backend
cargo build --bin qm
sudo install -m755 target/debug/qm /usr/local/bin/qm   # optional: put `qm` on PATH
qm --version                                           # → qm 0.2.0
```

(No PATH install? Use `target/debug/qm` wherever this doc says `qm`.)

**2 · Connect — is anything runnable?**

```console
$ qm doctor
ollama     http://localhost:11434       ✓ ready  v0.24.0  models: 9
llama_cpp  http://localhost:8081        ✗ unreachable
...
Next: qm run --backend ollama --model qwen2.5:3b
```

`doctor` probes all five backends and holds them to **runnable** (reachable + ≥1 model +
credential OK), not merely reachable. Anything wrong → a `[QM-…]` line on stderr with the exact fix
(`ollama serve`, `ollama pull …`, `check QM_API_KEY`), shown, never run.

**3 · First verdict — zero config**

```console
$ qm init
wrote qm.json (backend=ollama, model=qwen2.5:3b)
· [1/5] es_co_run_failing_test (prompt)
...
VERDICT: Ready   (ollama · qwen2.5:3b · easy-coding)
```

`init` auto-detects the first runnable backend, writes a non-secret `./qm.json`, and runs the suite.
From here a bare `qm run` needs **zero flags** (it reads `qm.json`), and the exit code is the verdict
— `qm run --fail-on notready` gates CI like a unit test.

**Remote GPU box instead of local?** No config file needed — env only:

```bash
export QM_BASE=https://vllm.internal:8000   # your endpoint
export QM_API_KEY=…                         # env or OS keychain — never an argument
qm doctor --backend vllm && qm run --backend vllm --model qwen3-32b
```

## Status (be honest about what ships)

This is an OSS tool built one verified command at a time. Today:

| Command | State | One-liner |
|---|---|---|
| `doctor`  | **shipped** | Diagnose every backend: reachable? models? credential? tool-calling? version? |
| `init`    | **shipped** | Auto-detect a running backend, write `qm.json`, and run the suite (zero config). |
| `run`     | **shipped** | Built-in tool-calling suite → a Ready/Conditional/NotReady verdict + exit code. |
| `test`    | **shipped** | Run a custom collection FILE (native + prompt) → a per-mode scoreboard + verdict. |
| `report`  | **shipped** | Re-assess a saved run against a readiness profile, offline (no backend). |
| `cliff`   | **shipped** | Context Stress Test: where does tool-calling collapse with prompt depth? |
| `validate`| **shipped** | Prove a collection/world is a reliable test — the gate `run`/`test` apply to uploads. |
| `prompt`  | **shipped** | Free-form generation: a system+user prompt with params, streamed to stdout (Workspace twin). |
| `verify`  | deferred | Signed-report tamper-evidence — out of scope for the local OSS tool (see below). |

`doctor`, `init`, `run`, `test`, `report`, `cliff`, and `validate` are implemented — the OSS CLI
surface. `verify` is
deferred (rationale in its section). This doc grows one section at a time as each lands, never ahead of
the code.

## Help & build notes

`qm --help` prints the command list; `qm <command> --help` the per-command flag reference. From a
checkout you can also run without building: `cargo run --bin qm -- <command> [flags]`. `qm` is a bin
target on the single `quantamind` crate (ADR 0001 — no workspace), so it shares the inference/eval
engine verbatim with the desktop app — the CLI and the GUI can never disagree about a verdict.

## Engines & ports

`qm` covers all five backends QuantaMind supports — three local, two remote:

| Backend | `--backend` | Kind | Port `doctor` probes | Notes |
|---|---|---|---|---|
| Ollama    | `ollama`    | local  | `11434` | Native `/api/version` + `/api/tags` + `/api/show` (tool-calling capability). |
| llama.cpp | `llama_cpp` | local  | `8081` then `8080` | QuantaMind's sidecar runs on **8081**; `8080` is the community `llama-server` default. OpenAI-compatible `/v1/models`. |
| MLX       | `mlx`       | local  | `8082` then `8080` | Apple-Silicon `mlx_lm.server`. OpenAI-compatible. |
| vLLM      | `vllm`      | remote | `8000` | OpenAI-compatible. Credential-classified (`/v1/models`). |
| SGLang    | `sglang`    | remote | `30000` | OpenAI-compatible. Credential-classified. |

`--base <url>` / env `QM_BASE` overrides the probed endpoint (targeted `--backend` only — a base URL
is backend-specific). A remote key comes from env `QM_API_KEY` or the OS keychain — **never argv**
(rule 7). A key is transmitted only over `https`/loopback; over plain http it is **withheld** and
`doctor` says so.

## Exit-code contract (a QuantaMind contract — gate CI on these)

These are ours, documented here — not `sysexits.h`. They are stable and shared across the `qm`
surface:

```
0  Ready / OK        10 Conditional      20 NotReady
2  bad args          3  nothing runnable / backend unreachable
11 Inconclusive (a probe couldn't run properly — CI should RETRY)
```

- `2` is standard usage-error (clap emits it on a parse failure — inherited for free).
- `3` is domain-specific ("nothing you can run"). For `doctor` it means **no runnable backend**; for
  `run`/`init` it means the backend was unreachable or the model isn't served (not a failing model).
- `0/10/20` are the `run`/`init` verdict codes (Ready/Conditional/NotReady); `doctor` never emits them.
- `11` (inconclusive) means `run` couldn't measure anything — the backend errored / timed out mid-run
  (distinct from a model that ran and failed, which is `20`). CI should **retry**, not hard-fail.

## Stream discipline

**stdout carries data; stderr carries diagnostics.** With `--json`, stdout is the report object and
*nothing else* — every `[QM-CODE] what — fix` line goes to stderr — so `qm doctor --json | jq` never
chokes on prose. In human mode the rendered report is stdout, the `[QM-CODE]` fix lines stderr.

---

## `doctor` — first-run diagnosis

The activation command: on a fresh machine it tells you exactly what to do next, in one command, with
no prompts. Ordered cheapest-first per backend — **reachable? → models? → credential? → native
tool-calling? → version** — and every failure carries the exact fix (shown, never run).

```
qm doctor [--backend <ollama|llama_cpp|mlx|vllm|sglang>] [--base <url>] [--model <name>] [--json]
```

| Flag | Meaning | Default / env |
|---|---|---|
| `--backend <kind>` | Check one backend. Omit to **scan all five**. | scan all |
| `--base <url>` | Endpoint for the targeted backend (with `--backend`). | env `QM_BASE` |
| `--model <name>` | Model to check native tool-calling against (Ollama). | env `QM_MODEL` |
| `--json` | Emit the machine-readable report on stdout (fixes still to stderr). | off |

**Runnable, not just reachable.** A backend counts as ready only when it is reachable **and** has ≥1
model **and** (remote) the credential resolved `Ok`. A reachable server with **zero models** — the
single most common first-run trap — is a loud finding, never a green line. `doctor` exits `0` iff at
least one backend is runnable, else `3`, so `qm doctor && qm run` short-circuits honestly.

**The three failure modes it keeps distinct** (opposite fixes — they must never swap):

| Situation | What `doctor` says |
|---|---|
| Server down / wrong port | `[QM-BACKEND-UNREACHABLE] … — start it: <command>` |
| Server up, key rejected (401/403) | `[QM-UNAUTHORIZED] <host> rejected the API key — check QM_API_KEY` |
| Key set but URL is plain http | `[QM-INSECURE-KEY] <host> — the key was withheld. Use https or drop the key.` |
| Reachable, no models | `[QM-NO-MODELS] <backend> is up but has no models — pull/serve one: ollama pull qwen2.5` |

**Exit:** `0` at least one runnable backend · `3` none runnable / unreachable · `2` bad args.

### Example — healthy scan (Ollama up, others off)
```
$ qm doctor
ollama     http://localhost:11434       ✓ ready  v0.24.0  models: 9
llama_cpp  http://localhost:8081        ✗ unreachable
mlx        http://localhost:8082        ✗ unreachable
vllm       http://localhost:8000        ✗ unreachable  credential: Unreachable
sglang     http://localhost:30000       ✗ unreachable  credential: Unreachable

Next: qm run --backend ollama --model qwen2.5:3b
$ echo $?
0
```
stderr is quiet when something is runnable (no fix-line spam for backends that are simply off). Point
at a specific backend to focus, and machine-read with `--json`:
```
$ qm doctor --backend ollama --model qwen2.5:3b --json | jq '.backends[0].native_fc'
"supported"
```

### Example — the first-run trap (reachable, zero models)
```
$ qm doctor --backend ollama          # ollama running, nothing pulled
ollama     http://localhost:11434       ! reachable  v0.24.0  models: 0

No runnable backend — fix the findings above, then re-run `qm doctor`.
# stderr:
[QM-NO-MODELS] ollama is up but has no models — pull/serve one: ollama pull qwen2.5
$ echo $?
3
```

---

## `run` — the readiness verdict

Runs a built-in tool-calling collection against one model through the same agentic engine as the
desktop app, and prints a **Ready / Conditional / NotReady** verdict with an exit code CI can gate on.

```
qm run [--backend <kind>] [--model <name>] [--collection easy-coding] [--profile general-agent]
       [--mode <prompt_based|native|both>] [--tier <easy|medium|hard|extreme>]
       [--thinking <lean|standard|deep>] [--k <n>] [--fail-on <conditional|notready|never>] [--json]
```

| Flag | Meaning | Default |
|---|---|---|
| `--backend <kind>` | ollama / llama_cpp / mlx / vllm / sglang. | qm.json, then interactive/ollama |
| `--model <name>` | Model to run. Env `QM_MODEL`. | qm.json, else interactive pick |
| `--base <url>` | Endpoint override (remote backends). Env `QM_BASE`. | qm.json / default port |
| `--collection <id>` | Built-in collection id. | `easy-coding` |
| `--profile <id>` | Readiness profile: `general-agent` / `rag-assistant` / `coding-agent`. | `general-agent` |
| `--mode <path>` | Calling path: `prompt_based`, `native` (function-calling), or `both`. `both` yields a verdict row per path. | `prompt_based` |
| `--tier <t>` | Difficulty-tier override — scales the per-turn token budget and the default `k` (Easy 5 … Extreme 24). | the collection's own tier |
| `--thinking <t>` | Reasoning-scratchpad budget: `lean` (off) / `standard` / `deep`. `standard`/`deep` are checked to actually take effect (Ollama: model capability via `/api/show`; llama.cpp/MLX/remote: a live probe for `reasoning_content`) — if reasoning won't happen, the run stops with a clear fix instead of silently behaving like `lean`. | `lean` |
| `--k <n>` | Override the **strict pass^k** run count (all `k` runs must pass). Higher = stricter. | the tier's default |
| `--fail-on <policy>` | Which verdict fails the *process*: `conditional` (Conditional→10), `notready` (Conditional tolerated→0), `never` (advisory→0). | `conditional` |
| `--json` | Emit the report as JSON on stdout (progress/notes to stderr). | off |

**Exit:** the verdict — `0` Ready · `10` Conditional · `20` NotReady — subject to `--fail-on`. An
`11` means the run **errored before it could measure anything** (backend fault / timeout) — a
`[QM-INCONCLUSIVE]`, CI should retry, never a definitive NotReady. A
`3` means the backend was unreachable or the model isn't served (**not** a failing model — that
distinction is the point); `2` is a bad arg, or a capability mismatch: `[QM-NATIVE-UNSUPPORTED]`
(`--mode native` on a model with no function-calling) or `[QM-THINKING-UNSUPPORTED]`
(`--thinking standard/deep` where reasoning won't actually happen — caught before the run on **every**
backend: Ollama would 400 every request; llama.cpp/MLX would silently ignore it and behave like
`lean`. The message names the per-engine fix, e.g. relaunch llama-server with `--jinja
--reasoning-format deepseek`).

**Interactive selection.** In a terminal, a missing `--model` (and no `qm.json`) opens a numbered
picker of the backend's served models; a missing `--backend` picks among the runnable ones. Over SSH
/ in CI / a pipe / with `--json`, there's no prompt — it fails fast with exit `2` (never blocks on
input).

### Example — a real run
```
$ qm run --backend ollama --model qwen2.5:3b
· [1/5] es_co_run_failing_test
  … (progress on stderr)
VERDICT: Ready   (ollama · qwen2.5:3b · easy-coding)
  [PromptBased] Ready  pass^k=0.80  runs=4/5

profile: general-agent
$ echo $?   # 0
```
A model that can't drive an agent fails honestly, naming the blocker — never a false pass:
```
$ qm run --backend ollama --model llama-3.2-1b-instruct:iq3_m
VERDICT: Not Ready   (ollama · llama-3.2-1b-instruct:iq3_m · easy-coding)
  [PromptBased] Not Ready  pass^k=0.00  runs=0/5
    ✗ pass^k 0.00 < 0.60 required
$ echo $?   # 20   (with --fail-on never → 0 + a [QM-NOTE], findings still shown)
```

## `init` — zero-config first run

Auto-detects the first **runnable** backend (reachable + has a model), writes a `qm.json` recording
it, then runs the suite — so `install → real verdict` takes one command and nothing typed. A later
`qm run` with no flags reads `qm.json`.

```
qm init [--json]
```

`qm.json` is a plain, non-secret record (a remote key stays in env/keychain, never the file):
```json
{ "backend": "ollama", "model": "qwen2.5:3b", "collection": "easy-coding", "profile": "general-agent" }
```

**Exit:** follows the verdict (`0/10/20`), or `3` with `[QM-NO-RUNNABLE]` when nothing is runnable
(run `qm doctor` to see what to fix).

```
$ qm init
wrote qm.json (backend=ollama, model=qwen2.5:3b)     # stderr
VERDICT: Ready   (ollama · qwen2.5:3b · easy-coding)
  [PromptBased] Ready  pass^k=0.80  runs=20/25
$ qm run          # no flags — reads qm.json
VERDICT: Ready   (ollama · qwen2.5:3b · easy-coding) …
```

## `test` — run YOUR collection (per-mode scoreboard)

Same engine as `run`, but against a **collection file you provide** and defaulting to `--mode both`,
so you get a native-vs-prompt scoreboard for your own eval.

```
qm test --collection <file.json> [--backend <k>] [--model <m>]
        [--mode both] [--tier <t>] [--thinking <t>] [--k <n>] [--fail-on <p>] [--junit <path>] [--json]
```

**Collection file** — JSON, auto-detected between two shapes (size-capped at 1 MB, schema-validated):
1. a **v2 collection object** `{ "name", "domain", "tier", "tasks": [ … ] }` — the format the desktop
   Tests page authors and the built-ins use (multi-step `agent_loop` tasks; this is what the readiness
   verdict scores);
2. a raw **`ToolTask[]` array**.

Other formats (CSV, JSONL, BFCL, τ-bench, OpenAI-evals) are **not** parsed by the CLI — convert to one
of the two JSON shapes first. (`qm run --collection <file>` accepts a file too; `test` just defaults to
both modes and prints the scoreboard.)

Flags mirror [`run`](#run--the-readiness-verdict) (`--tier`/`--thinking`/`--k`/`--fail-on`/`--junit`/
`--json`); `--mode` defaults to `both`. Exit codes are the same contract.

### Example
```
$ qm test --collection ./my_suite.json --backend ollama --model qwen2.5:3b --k 1
· [1/2] es_co_run_failing_test (native)
  … (progress on stderr)
VERDICT: Not Ready   (ollama · qwen2.5:3b · my_suite.json)

mode          pass^k  tasks   steps  effort   top-error
NativeFc      0.00    0/2     2.0    —        reported_in_prose_calls=2
    ✗ pass^k 0.00 < 0.60 required
PromptBased   1.00    2/2     2.0    49       none

profile: general-agent
```
The scoreboard makes the native-vs-prompt split obvious — here the 3B's native tool-calling reports in
prose while its prompt-based path passes. A bad/missing/malformed file exits `2` with a clear
`[QM-BAD-COLLECTION]` (the path is redacted per rule 7f).

## `report` — re-assess a saved run, offline

Score a **saved run** against a readiness profile without touching a backend — so you can hold one run
up to many bars (a strict launch gate, a lenient smoke bar) in milliseconds.

```
qm run  --model <m> --save-report run.json      # or: qm test … --save-report run.json
qm report --report run.json --profile <id|file.json> [--fail-on <p>] [--junit <path>] [--json]
```

`--save-report <path>` (on `run` and `test`) writes the **raw run report** (the internal `BatchReport`,
re-loadable). `qm report` reloads it, re-assesses against `--profile` (a built-in id **or** a
`ReadinessProfile` `.json`), and prints the same verdict as `run` — but offline, and against whatever
bar you name. No inference, no endpoint.

**Profile file** (all fields shown; `required_tier` is `easy|medium|hard|extreme`):
```json
{ "id": "strict", "name": "Strict launch gate", "min_pass_k": 0.9,
  "max_avg_steps": null, "max_ms_per_step": null, "min_context_tokens": null,
  "forbid_infinite_loop": true, "forbid_hallucinated_completion": true,
  "require_full_vram": false, "require_native_fc": false, "required_tier": "easy" }
```

The desktop app's **Agent Report** page writes this file for you: its "Equivalent CLI command"
preview emits a `printf '{…}' > profile.json && qm report …` chain carrying the exact thresholds
active on the page — so the CLI verdict always matches the page, even for an edited profile.

### Example — one run, two bars
```
$ qm report --report run.json --profile general-agent    # min_pass_k 0.6
VERDICT: Ready   (ollama · qwen2.5:3b · easy-coding)
  [PromptBased] Ready  pass^k=0.80  runs=4/5

$ qm report --report run.json --profile strict.json       # min_pass_k 0.9
VERDICT: Not Ready   (ollama · qwen2.5:3b · easy-coding)
  [PromptBased] Not Ready  pass^k=0.80  runs=4/5
    ✗ pass^k 0.80 < 0.90 required
$ echo $?   # 20
```

**Exit:** the verdict (`0`/`10`/`20`, subject to `--fail-on`). `2` on a bad/missing report or profile
file (`[QM-BAD-REPORT]` / `[QM-BAD-PROFILE]`, path redacted).

## `cliff` — Context Stress Test

Ramps prompt depth toward `--max-tokens` and classifies where tool-calling collapses — the same
Tauri-free engine the desktop Audit tab drives, prompt-based, **greedy (temp 0)** so the same
(model, collection) reproduces the same verdict.

```
qm cliff [--backend <k>] [--model <m>] [--collection <id|file>]
         [--max-tokens 4096] [--steps 4] [--source <corporate_policy|system_logs|financial_ledger>] [--json]
```

Output: one line per rung (`~N tok · accuracy X% (passed/trials)` — the tally shown only when
measured) then a `STATUS:` line. A rung that would exceed the context window is dropped, never
scored (the verdict uses only real measurements).

**Exit:** `0` no-cliff · `10` collapsed · `11` inconclusive (sample too small to resolve a cliff
from noise — add tasks/repeats, don't trust a coin flip) · `20` broken baseline (fails at the
smallest context — a tool-call failure, not a context limit) · `2`/`3` as usual.

## `validate` — prove the test before trusting it, and MCP worlds

An eval is only as honest as its answer key. `validate` proves a collection is a **reliable test**
before any model runs it — and the same pipeline gates `run`/`test` automatically on every uploaded
file (**an invalid collection can never start testing**; there is deliberately no bypass flag).

```
qm validate [--collection <id|file.json>] [--live-world <true|false>] [--json]
```

What it proves, per task:
- **structural** — schema, known tools, consistent agentic spec (always ran at load).
- **reachable** — a scripted perfect agent replaying the oracle reaches the end state (sandbox tasks).
- **discriminating** — a do-nothing agent FAILS. This is the check the field keeps re-learning the
  hard way: a 2026 τ-bench audit measured a literal do-nothing agent at 38% pass^k, and SWE-bench had
  to ship "Verified" after unsolvable tasks poisoned results. A task a do-nothing agent passes proves
  nothing.
- **world checks (MCP tasks)** — static: vacuous oracle (asserts nothing), contradictory oracle
  (present ∩ absent), escaping/absolute seed paths; **live** (default, needs `npx`): spawn the REAL
  world, then grade the oracle against the untouched seed — it must fail, or the world is vacuous.

**Exit:** `0` valid · `10` warnings only · `20` invalid (fix the ✗ findings) · `11` worlds couldn't be
live-checked (npx/sqlite3 missing — install hint printed) · `2` bad file.

### World files — author a real-tool test in JSON

A third auto-detected collection shape (alongside the v2 object and raw `ToolTask[]`): an array of
world tasks, the same shape the desktop MCP builder authors. Each spawns a REAL MCP server
(`@modelcontextprotocol/server-filesystem` / `mcp-server-sqlite-npx` via `npx`) scoped to a fresh
throwaway sandbox per run; grading reads the **end state of the world, never the model's words**.

```json
[{ "name": "summarize-notes",
   "instruction": "Read notes.txt, then create summary.md containing the word 'alpha'.",
   "world":  { "type": "fs", "files": [{ "path": "notes.txt", "content": "Project alpha shipped." }] },
   "oracle": { "assert_present": ["summary.md"], "assert_content": [["summary.md", "alpha"]] } },
 { "name": "insert-user",
   "instruction": "Insert a user named alice into the users table.",
   "world":  { "type": "db", "setupSql": "CREATE TABLE users (name TEXT, email TEXT);" },
   "oracle": { "assert_contains": [["SELECT name FROM users", "alice"]] } }]
```

Then simply: `qm run --collection ./worlds.json --model <m>` — the gate validates, each of the `k`
runs gets a byte-identical fresh world, the model acts through real tools, the oracle grades the
real files/rows. Requirements: `npx` (Node.js) for every world; `sqlite3` additionally for db worlds
— missing deps are reported with the install command before any model time is spent
(`[QM-WORLD-DEPS]`). A server that dies mid-run is classified **Inconclusive (11)** — a server
fault, never a fabricated model failure. Scratch worlds are cleaned even after a `kill -9`
(orphan sweep on the next world use).

## `prompt` — free-form generation (the Workspace twin)

The one non-eval command: run a system+user prompt against a model with inference params and stream
the output — the headless equivalent of the Workspace Run button (same inference path).

```
qm prompt [--backend <k>] [--model <m>] [--system '…'] [--user '…']
          [--temperature 0.7] [--top-p 0.9] [--top-k 40] [--num-predict 512]
          [--repeat-penalty 1.1] [--seed 42] [--num-ctx 8192]
```

The user prompt comes from `--user` or, when omitted, **stdin** (pipe it, or type + Ctrl-D). Tokens
stream to **stdout**; `[QM-*]` diagnostics + a `[QM-DONE] N tokens` summary go to stderr, so
`qm prompt … | tee out.txt` captures only the generation. Exit `0` ok · `3` unreachable /
model-not-served · `2` bad args/params.

```
echo "Summarize this in one line: …" | qm prompt --backend ollama --model qwen2.5:7b --temperature 0.7
qm prompt --model qwen2.5:7b --system "You are terse." --user "Name three primary colors."
```

## Inference params & extra eval knobs

`run`, `test`, and `cliff` accept the **same 7 global params** as `prompt`
(`--temperature --top-p --top-k --num-predict --repeat-penalty --seed --num-ctx`) — they mirror the
GUI's global params exactly (`max_tokens` → `--num-predict`). Eval is **greedy (temp 0) by default**
for reproducibility; pass `--temperature` (etc.) to sample, matching the GUI. `cliff` is greedy too
unless you pass params, and gains `--mode native|prompt_based` (default prompt-based) to probe the
native tool-calling path. `run`/`test` also gain `--max-steps N` (per-turn step cap) and `--decoy N`
(decoy tools injected per task) — the last two UI controls that had no flag.

## Interactive pickers

In a terminal, omitting a value opens a numbered picker (over SSH/CI/pipes there is never a
prompt — omitted values fall back to safe defaults):

- `--model` / `--backend` — pick from the probed backend's served models (as before).
- `--collection` (`run`/`cliff`) — pick from the **27 built-in collections**, listed with their
  tier (`easy` / `medium` / `hard` / `extreme` + the boundary/safety sets) and domain. Non-TTY
  default: `easy-coding`.
- `--thinking` (`run`) — pick the thinking tier: `lean` (reasoning OFF) / `standard` (~2k
  scratchpad) / `deep` (~8k). Non-TTY default: `lean`. `standard`/`deep` remain guarded per
  model+server (see `[QM-THINKING-UNSUPPORTED]`).

## `verify` — deferred (out of OSS scope)

`verify` would check a **cryptographically signed** report's integrity — tamper-evidence for a report
someone shares or publishes. It is **deliberately not built** in the OSS core:

- Its value is a *trust boundary* (a report moving between parties). This local, single-user tool
  doesn't have one yet — a signature you both generate and verify on your own machine proves nothing.
- It needs signing infrastructure (an Ed25519 dependency + key management) that the lean OSS surface
  intentionally dropped (it lived on the enterprise serving-ops commands), and the project's scope keeps
  at-rest crypto as a *seam*, not a built feature.

It becomes worth building the day reports are shared/published (a real recipient to protect). Until
then, shipping a signature nobody needs would be security theater.

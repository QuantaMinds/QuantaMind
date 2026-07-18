# QuantaMind CLI (`qm`) — reference

`qm` is the **headless** face of QuantaMind: the same agent-readiness engine as the desktop app,
driven from the terminal for first-run setup, CI gating, air-gapped runs, and scripting. It talks
directly to your local or remote inference server — no GUI, no phone-home. Every command prints
human text, or a machine-readable object with `--json`.

## Status (be honest about what ships)

This is an OSS tool built one verified command at a time. Today:

| Command | State | One-liner |
|---|---|---|
| `doctor`  | **shipped** | Diagnose every backend: reachable? models? credential? tool-calling? version? |
| `init`    | **shipped** | Auto-detect a running backend, write `qm.json`, and run the suite (zero config). |
| `run`     | **shipped** | Built-in tool-calling suite → a Ready/Conditional/NotReady verdict + exit code. |
| `test`    | **shipped** | Run a custom collection FILE (native + prompt) → a per-mode scoreboard + verdict. |
| `report`  | **shipped** | Re-assess a saved run against a readiness profile, offline (no backend). |
| `verify`  | deferred | Signed-report tamper-evidence — out of scope for the local OSS tool (see below). |

`doctor`, `init`, `run`, `test`, and `report` are implemented — the OSS CLI surface. `verify` is
deferred (rationale in its section). This doc grows one section at a time as each lands, never ahead of
the code.

## Running it

```bash
# from a checkout (debug build):
cargo run --bin qm -- <command> [flags]

# or build once and use the binary:
cargo build --bin qm             # → target/debug/qm
target/debug/qm <command> [flags]

target/debug/qm --help           # top-level help
target/debug/qm doctor --help    # per-command help
```

`qm` is a bin target on the single `quantamind` crate (ADR 0001 — no workspace), so it shares the
inference/eval engine verbatim.

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

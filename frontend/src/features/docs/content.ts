/// The Docs page content — friendly, task-oriented user guides (authored, not sourced from the
/// dense engineering docs/*.md). Each page is a markdown string rendered by `DocMarkdown`.
/// Grouped into collapsible sidebar sections, mirroring a docs-site nav tree.

import { referenceToDocSection } from "./reference";

export type DocPage = { id: string; title: string; description: string; body: string };
export type DocSection = { id: string; title: string; pages: DocPage[] };

const gettingStarted = `
# Getting started

QuantaMind is a **local** benchmarking and evaluation workbench for open-weight LLMs. Everything
runs on your own machine — you pick a backend (the program that actually runs the model), point it
at a model, and QuantaMind measures how well and how fast it performs. No data leaves your computer.

## The layout

The app is organized into tabs across the top:

- **Workspace** — a scratchpad to chat with the selected model and see live token metrics.
- **Tests** — run agentic / tool-calling evaluations over a collection of tasks.
- **Agent Report** — a plain-language verdict on whether a model is ready for agentic work on your hardware.
- **Latency** — token-timing, VRAM use, and context-ceiling estimates per model.
- **Analysis** — compare runs side by side.
- **Models / Downloads** — install and manage local models.
- **Settings** — backends, storage, and remote endpoints.

## Before you run anything

You need two things: a **backend** running, and a **model** installed for it.

1. Pick a backend in the header dropdown (top-left). The dot next to it is green when that backend's server is up.
2. Make sure a model for that backend is installed (see **Adding models**).
3. Select the model, then head to **Tests** or **Workspace**.

> [!TIP]
> The little colored dot in the header is your backend's health. Green = the server is running and reachable. If it's grey/red, start the server (Ollama, llama.cpp, or MLX) — the **Choosing a backend** guide covers each.

## Next steps

- **Choosing a backend** — which engine to run and when.
- **Adding models** — get a model onto your machine.
- **Running your first test** — measure a model end to end.
`.trim();

const choosingBackend = `
# Choosing a backend

A *backend* is the server that loads and runs the model. QuantaMind talks to whichever one you
select in the header. You run **one at a time** — the whole app is scoped to the selected backend.

## The three local backends

| Backend | Best for | Notes |
| --- | --- | --- |
| **Ollama** | The easiest start | One-command install; manages its own models |
| **llama.cpp** | GGUF weights + fine control | Bundled server; great for right-sizing context |
| **MLX** | Apple Silicon only | Uses the unified-memory GPU; prompt-based only |

## Starting a backend

- **Ollama** — install from ollama.com, then it runs in the background. QuantaMind detects it automatically.
- **llama.cpp** — QuantaMind ships the server; it starts when you select a GGUF model.
- **MLX** — Apple-Silicon only; QuantaMind launches \`mlx_lm.server\` for you.

The header dot turns green once the server responds.

## Native tool-calling needs a capable model

This is the most common surprise. **Native tool-calling** (the model emits structured tool calls)
only works if the model advertises a \`tools\` capability. Many hand-imported quantized models do
**not** — they only advertise \`completion\`.

> [!IMPORTANT]
> If you select **Tool-Calling (native)** on a model that can't do it, the run will tell you
> "Native unsupported" instead of silently returning nothing. Either tick **Prompt-based** (which
> asks the model to produce tool calls as text and works on any model), or pick a tools-capable
> model such as a \`qwen3.5\` build.

## Thinking models

Some models "think" (produce a reasoning scratchpad) before answering. QuantaMind detects a
\`<think>\` block in the output automatically and strips it before scoring, so a reasoning model is
never penalized for its scratchpad — even if you didn't flag it as a thinking model.

## Next steps

- **Adding models** — install a model for your chosen backend.
- **Running your first test** — put the backend to work.
`.trim();

const addingModels = `
# Adding models

Models live in the **Downloads** and **Models** tabs. What you install depends on your backend.

## By backend

- **Ollama** — pull models with Ollama itself (e.g. \`ollama pull qwen3.5:9b\`); they appear in QuantaMind automatically.
- **llama.cpp** — drop \`.gguf\` files into the shared weights folder (shown on the Downloads page), or download them from the **Hugging Face** tab.
- **MLX** — MLX-format model folders go in the MLX models directory.

## Downloading from Hugging Face

1. Open **Downloads → Hugging Face**.
2. Type a search (e.g. "qwen3.5 gguf").
3. Pick a file and download — it lands in your weights folder and shows up in the model picker.

> [!TIP]
> Quantized files (\`q4_k_m\`, \`iq3_m\`, …) are smaller and faster but slightly less accurate. For a
> 16 GB machine, a 7–9B model at \`q4_k_m\` is a good balance.

## Checking what's installed

The **Models** tab lists everything QuantaMind can see, grouped by backend, with sizes. The
**Downloads** page also shows disk usage and a **Clear cache** button.

## Next steps

- **Running your first test** — evaluate a model you just installed.
`.trim();

const workspace = `
# Working in the Workspace

The **Workspace** is your home base: pick a model, write a prompt, run it, and watch tokens stream
in with live performance metrics. It's the quickest way to feel out a model before you commit to a
full test run.

## The model bar

The bar across the top drives the *whole app* — every page runs against the globally-selected model,
so there's no per-page model choice to keep in sync.

- **Model dropdown** — the selected model. For **Ollama** you can multi-select **2+ models**, which
  turns Run into a side-by-side **Compare** (see **Comparing models**).
- **Gear (⚙)** — a temperature popover (0.0–2.0), persisted per model.
- **Stop (◼)** — kills the backend server; the header health dot flips red immediately.

> [!TIP]
> On a fresh machine the dropdown is replaced by an "Ollama is not running" card. On macOS it offers
> **Start Ollama** / **Install Ollama**; on Windows and Linux, start Ollama yourself and click
> **Check again**.

## System + user prompt

Two editors — an optional **System** prompt and the **User** prompt — mirror how the model is
actually called, so what you test here matches what your app sends in production. **Run** streams
tokens as they generate; a large model may show "Loading model…" for up to ~30s on the first run
while weights load into memory. Cancelling mid-stream stops cleanly and keeps what was produced.

## Run metrics

After a run you get three *measured* numbers (never estimated, never a chars÷4 guess):

- **TTFT** — time to the first streamed token (how long until it starts).
- **tokens/s** — steady-state throughput, measured from first token to last.
- **token count** — the backend's own counter.

For per-token *forensics* (where each millisecond went), open the **Latency** tab.

## Prompt templates

The template picker pastes a known-good prompt skeleton (summarize, extract JSON, tool-call, …) into
the User box, which you then edit freely — a faster start than re-deriving the shape each time.

## Next steps

- **Comparing models** — run one prompt across several models at once.
- **Running your first test** — score a model on a whole collection of tasks.
`.trim();

const analysis = `
# Comparing models side by side

The **Analysis** tab runs *one prompt across several models* and shows their output and speed next to
each other — the only honest way to choose between models for a task.

## Starting a compare

Select **2+ models** in the Workspace picker (Ollama), write your prompt, and Run. Each model gets
its own **column** that streams independently — a slow model never holds up the others. Directly
below the answers you also get the full per-token **latency** panels (the same ones the Latency tab
shows), so quality and speed sit together.

## Run strategies

Local models are constrained by RAM, so you choose how they share your machine:

| Strategy | What it does | Best when |
| --- | --- | --- |
| **Sequential** | One model at a time; each is evicted before the next loads | Limited RAM |
| **Sequential (skippable)** | Same, with a **Skip** button per row | You want to bail on a slow one |
| **Parallel** | All models load at once | You have the RAM and want speed |

## Reading the results

- **Compare columns** — each model's raw streamed output.
- **Metrics chart** — throughput and TTFT bars from the same measured per-run numbers. A metric the
  backend didn't report is drawn as *absent*, never a fake 0.
- **Output diff** — pick two columns for a word-level diff that highlights exactly where two answers
  diverge.
- **Export** — save the whole comparison as Markdown (human-readable) or JSON (the machine-readable
  analysis document).

> [!TIP]
> The context-ceiling meters here include the **weights-fit chip** and (on Apple Silicon) the
> GPU-addressable line — so the capacity-vs-capability read is available right under the answer, not
> only on the Latency tab.

## Next steps

- **Latency & memory of a Test run** — the timing/memory forensics behind these panels.
`.trim();

const firstTest = `
# Running your first test

The **Tests** tab runs a *collection* of tasks against one or more models and scores them. This is
the core of QuantaMind.

## Prerequisites

- A backend running (green dot) and a model selected.
- For **native** tool-calling: a tools-capable model (see **Choosing a backend**).

## Steps

1. Go to the **Tests** tab.
2. Pick a **collection** (a bundled set of tasks, or one of your own).
3. Choose the method: **Tool-Calling (native)** and/or **Prompt-based**.
4. Click **Run Batch**.
5. Watch the scoreboard fill in as each task completes.

## Reading the scoreboard

Each model gets a row. The key columns:

- **Pass^k** — the fraction of tasks the model passed on *every* one of its k attempts (the strict, headline number).
- **Effort** — mean output tokens on the runs that *succeeded* (the token cost of getting it right).
- **Tokens/Task** — the *amortized* cost: total tokens ÷ completed tasks, **including tokens burned on failed runs**. It's always ≥ Effort, and the gap shows how much a flaky model really costs. Lower is better; "—" when nothing completed.
- **Top Error** — the most common failure mode (e.g. "Loop Cap", "Bad Dialect", "No Output").
- **Native** vs **Prompt** columns show each method separately — they're never averaged together.

> [!NOTE]
> **Effort vs Tokens/Task.** Effort is the best case (successes only); Tokens/Task charges the "waste tax" of retries and failures. A model that usually works but occasionally loops for thousands of tokens looks cheap on Effort and expensive on Tokens/Task — that gap is the real bill.

> [!NOTE]
> If a native run shows **"Native unsupported"** or **"Backend error"** in red, the model/template
> couldn't do native tool-calling (or the server errored). That's honest — it's not a zero score,
> it means the run couldn't happen. Switch to Prompt-based or a capable model.

## Safety & Boundaries (Category K)

Run one of the built-in **boundary-** collections (\`boundary-healthcare\`, \`boundary-banking\`,
\`boundary-coding\`) and a **Safety & Boundaries** panel appears below the matrix. It measures two
things together:

- **Resistance** — how often the model *refused* a prompt-injection / unsafe tool call.
- **Over-refusal** — how often it wrongly *refused a legitimate* request (the false-positive control).

The gate passes only when resistance is high **and** over-refusal is low — a model that refuses
everything scores perfect resistance but fails on over-refusal, so it can't look "safe" by being
useless. When a config does take an unsafe action, the panel attributes it: the *model* followed
the injection, or the served *config* let its context window evict the safety instruction.

> [!NOTE]
> This is a **floor on vulnerability, not a ceiling on safety** — it tests a fixed set of known
> injections; adaptive attacks do materially better. Config-attribution applies only to the
> context-truncation case; injection resistance broadly needs an out-of-band guardrail layer this
> tool doesn't provide.

## Realistic tool output (payload noise)

Some collections (e.g. \`noisy-extraction\`) return tool results the way real APIs do — the answer
buried under metadata, timestamps, and pagination. They test whether a model can pull the right
field out of messy JSON, not just a clean one-field blob.

## If Run Batch seems to do nothing

A run that can't start now shows its reason right next to the button. The most common cause is
picking native-only tool-calling on a model without the \`tools\` capability — tick **Prompt-based**
and re-run.

## Next steps

- **Reading the Agent Report** — turn scores into a go/no-go verdict.
- **Latency & context** — measure speed and how much context fits.
`.trim();

const agentReport = `
# Reading the Agent Report

The **Agent Report** turns raw test scores into a plain-language answer: *is this model ready to do
agentic work on this machine?*

## What it shows

- **Executive verdict** — a one-line readiness call, plus the tier tested and your hardware class (e.g. "Mainstream (16 GB RAM)").
- **GPU right-sizing** — how the model's memory footprint fits your GPU / unified memory, and how much you'd save at a smaller quant.
- **Per-tier results** — how the model did as tasks get harder.

## GPU right-sizing

This section compares quantizations of the same model family, so you can see the accuracy-vs-memory
trade-off. It appears once **two or more** quants of a family have been assessed — with only one,
you'll see a hint to assess another.

> [!TIP]
> On Apple Silicon there's no separate VRAM — the GPU shares system memory, so the report shows a
> single unified-memory figure (e.g. "16 GB RAM" *is* your GPU budget).

## Where the GPU size shows

- **Settings → Hardware** — the authoritative GPU / device-memory readout.
- **Agent Report** — the hardware class line ("… (16 GB RAM)").
- **Latency** — the per-model VRAM bar ("… of 16.0 GB").

## Next steps

- **Latency & context** — the timing and memory detail behind the verdict.
`.trim();

const latency = `
# Latency & memory of a Test run

The **Latency** tab answers one question: **what did the run you launched on the Tests tab cost on
this machine** — per task, live while it streams. (Workspace-prompt per-token timing — TTFT,
token-gap charts — lives under **Analysis**, right below each answer.)

A rule the whole page follows: every number names how it was obtained — *measured*, *computed from
measured tokens*, *estimated (formula)*, or the model tag's unverified *claim*. Anything the backend
doesn't report shows **"Not available"**, never a guessed value.

## Per-task cards

Each task gets a card per **pass** — **native FC** (the model's tool-calling API) and prompt-based
are different eval methods, so their costs are shown separately, never blended.

- **Prefill / Decode** — server-reported time reading the prompt vs generating tokens, summed over
  all Pass^k runs. The little bar track shows the same split per step.
- **Output tokens** — tokens generated across all runs.
- **Thinking tokens** — on **llama.cpp**, a *measured* split: the reasoning channel tokenized with
  the model's own tokenizer. On **Ollama** the API reports one combined count, shown as
  "*(no split)*" — no honest split exists there today.
- **Cache hits** — prompt tokens served from the server's prefix cache instead of recomputed.
  Measured on **llama.cpp** (it's why steps after the first take ~1s); **Ollama** reports no such
  count → "Not available".
- **Peak context** — the fullest a single run's window got. Cache-hit totals can exceed it: hits
  accumulate across runs, the peak is one moment.
- **Wall** — the task's whole Pass^k batch on a real clock, including time between turns.

## Memory for this run

- **Model in memory** — Ollama: resident size from /api/ps (weights **plus** the context buffer it
  reserves at load — that's why it reads above the raw weight file). llama.cpp: the GGUF's size at
  launch (it reports no resident split).
- **KV cache at this run's peak** — the headline: what *this* run's deepest context cost, with the
  same tokens priced at f16 / q8_0 / q4_0 underneath (cache precision is a *launch* setting).
- **Server process RSS** — a diagnostic: the server process's resident RAM at step end. Whole
  process (weights + leftovers), and GPU-wired buffers may be missing — so it can legitimately read
  *below* the model's in-memory size.
- **Fit verdict** — model + this run's KV vs your device's memory pool ("Tight" above 70%). A
  planning estimate, not a measured OOM point. If a task actually dies of out-of-memory, the panel
  names it and computes the context ceilings (f16/q8_0/q4_0 KV) that would fit instead.

## Context: usage vs capacity

Two different things, two different displays:

- **Context window budget** ("757 / 16384 ctx (5%)") — *usage*: the peak tokens of a single run vs
  the window the run launched with. Tokens are precision-independent — f16/q8/q4 change the *bytes*
  each cached token costs, never this count.
- **Context ceiling by KV-cache precision** — *capacity*: the largest window this machine could
  hold at each precision. **f16** is the default; **q8** ≈ 2× at negligible quality cost; **q4** ≈
  4× with a real quality cost and often slower at long context. On **Apple Silicon** the budget is
  the GPU's **measured Metal limit**, shown as the **GPU-addressable** line ("~11.8 GB of 16.0 GB
  usable by the GPU") — not the whole pool, since macOS only lets the GPU wire down ~66–75% of
  unified memory.
- **Weights-fit chip** — right above the bars, a coloured verdict on whether the *model itself*
  fits under that GPU limit: **✓ Fits**, **⚠ Tight** (weights leave little room for context), or
  **✕ spills to CPU/swap** (the weights alone exceed the limit → very slow). It answers the
  question a big ceiling can't: a 100K ceiling is meaningless if the model doesn't even load on the
  GPU. (Off Apple Silicon, where the limit isn't measured, no chip shows — never a guess.)

> [!IMPORTANT]
> **Capacity, not capability.** These bars measure *memory only* — not speed (long context is slow
> to prefill even when it fits), and not whether the model runs on the GPU (the fit chip answers
> that). "Could fit in memory" is not "will run well."

> [!NOTE]
> The ceiling bars need the model's dimensions and size. For **llama.cpp**, start the server from
> the app (an externally started one can't report its launch settings); for **Ollama** they come
> from the model's reported metadata.

## Next steps

- **Troubleshooting** — if something's not measuring or running.
`.trim();

const customCollections = `
# Building your own test collection

Your tasks — not a generic fixture — are what tell you whether a model is ready for *your* app. Custom
collections are first-class: build them by hand or bulk-load from a spreadsheet, on the **Tests** tab
under the **Custom JSON** source.

## Two ways to build

- **Collection editor** — author tasks by hand: a name, domain, tier, and a list of single-turn
  tool-call tasks. Tool schemas live in their own box and apply to every task.
- **CSV import** — bulk-load single-turn tool-call tasks from a spreadsheet.

## The CSV format

Exactly four columns, in order:

| Column | Meaning |
| --- | --- |
| \`id\` | unique task id |
| \`prompt\` | the user request |
| \`expected_tool\` | the tool the model should call — **leave empty for an abstain task** (correct behavior is *no* call) |
| \`expected_args\` | the arguments, as a JSON object |

One row = one task. The dialog **validates live** — wrong header order, bad args JSON, an unknown
tool, or a duplicate id are flagged per row, and **Import stays disabled until it's clean**.

> [!TIP]
> An **abstain task** (empty \`expected_tool\`) tests restraint — the model passes by *not* calling a
> tool it shouldn't. Mix these in so a trigger-happy model can't score full marks.

## Scoring

Custom collections run through the same engine as the built-ins: **tool-call composite accuracy**
(right tool? right args? silent when it should be?) plus **Pass^k**, **Avg Steps**, **Effort**, and
**Schema Resilience**. Scoring is *structural* — it matches the call name and args, never executes the
tool. For tasks that need *real* tools and real end-state grading, use **MCP** instead.

## Headless

Everything you author here runs from the terminal: \`qm test --collection ./my_suite.json\` — same
schema, same engine, an exit code you can gate CI on.

## Next steps

- **Testing with MCP servers** — grade against real tools and real world-state.
- **The qm command-line tool** — run your collection headless.
`.trim();

const mcp = `
# Testing with MCP servers

Most collections score a model against *simulated* tools. **MCP** (Model Context Protocol) tests it
against **real tools** — a real filesystem, a real SQLite database — and grades **what actually
happened in the world**, not what the model *said* it did. It's the most honest agentic test
QuantaMind runs.

MCP lives on the **Tests** tab: at the top, switch the data source from **Built-in** / **Custom JSON**
to **◉ MCP**. The page becomes the MCP connect-and-build flow, reusing the same scoreboard and **Run
Batch** button as every other collection.

## 1. Connect a server

An MCP server is a small local program that exposes tools over a standard protocol. In the connect
panel, add one with an **id**, a **command** (defaults to \`npx\`), and **args**. Two quick-add chips
cover the common cases:

- **filesystem** → \`npx -y @modelcontextprotocol/server-filesystem\` — append a directory the server
  may touch.
- **sqlite** → \`npx -y mcp-server-sqlite-npx\` — QuantaMind supplies a scratch database path for you.

Both need **Node / npx** installed. **Connect** runs a quick probe and reports **"✓ N tools
discovered"** or a loud red error — so a bad command fails at setup, not mid-run.

> [!NOTE]
> The transport is local **stdio** (a child process on your machine) — nothing goes to the cloud, and
> server output (tool descriptions, results) is always rendered inert, never executed.

## 2. Author a task — two doors

### Test World (scored)

The **QuantaMind Test World** builder is three sections that *are* the task:

1. **Task** — a name and a plain-language instruction ("create result.txt containing DONE").
2. **Set up the world** — a **temp folder** (seed files + contents) or a **temp database** (setup
   SQL). Fresh and disposable every run.
3. **Check the result (the answer key)** — for files: which must be **present**, **absent**, or
   **contain** a substring; for a database: \`SELECT … :: expected\` assertions.

Saving adds the task to the sidebar (you can also paste task JSON directly). Either way the task is
**auto-validated**: the answer key must be *solvable* (a perfect agent passes) **and** *discriminating*
(a do-nothing agent fails). A vacuous or contradictory key is rejected with named findings — because a
broken answer key makes every score a lie.

### Bring-Your-Own (diagnostic)

The **Bring-Your-Own** door is just an instruction + one of your connected servers, with **no answer
key**. It doesn't pass or fail — it reports **"schema-valid X/Y"** (did the model emit well-formed
calls to your real tools?). Handy for smoke-testing a server; kept out of the pass-rate aggregate so
scores are never blended.

Because BYO targets a **real** server, tool calls are **deny-by-default**: they're graded but **not
executed** until you tick **"Allow tool execution"** on the builder. That opt-in is your explicit
approval — leave it off to inspect what the model *would* do, turn it on (against a throwaway target)
to actually run the calls.

## 3. Run and read

Pick your model and iterations (**k**) in the global header, then **Run Batch**. For each Test-World
task, per run QuantaMind: seeds a fresh sandbox → spawns the real server scoped to it → lets the model
drive the tools → **grades the world's end-state against your answer key** → throws the sandbox away.
A transcript that claims "done" counts for nothing; only the files/rows do.

The verdict is **Pass^k**, and it's strict — a task is *ready* only if **every** one of its k runs
passed. One lucky pass is not readiness; failed runs record which checks they missed.

## Safety

- **Controlled worlds are disposable and isolated** (a brand-new temp folder/DB per run), so their
  tool calls auto-approve — you can't gate k automated runs on human clicks.
- Seed paths are confined by \`fs_guard\` (no \`..\` escapes or symlink tricks); secrets live in the OS
  keychain, never on disk; tool definitions are hash-pinned, so a server that swaps its tools between
  runs trips the pin.
- **Your own (Bring-Your-Own) servers are deny-by-default.** A BYO run won't execute the model's tool
  calls against your real server until you tick **"Allow tool execution"**. Even then, confinement is
  only as tight as you scoped the server — so point the filesystem server at a *throwaway* directory,
  never your home folder.

## Prove it before you trust it

Run **\`qm validate --collection ./worlds.json\`** (or let \`qm run\` / \`qm test\` do it automatically) to
put a world file through the same gate the UI uses — structural checks plus a live spawn where a
do-nothing agent must fail the real answer key. Missing \`npx\` / \`sqlite3\` is reported as inconclusive
with the exact install command, never a fake failure.

## Next steps

- **Building your own test collection** — simulated-tool tasks (no server needed).
- **The qm command-line tool** — run and gate all of this headless.
`.trim();

const cli = `
# The qm command-line tool

\`qm\` is the headless face of QuantaMind — the *same* evaluation engine as the app, driven from a
terminal and returning an exit code you can gate CI on. Every control on the Tests page maps to a flag.

## Build it

\`qm\` is a second binary on the same crate:

\`\`\`
cargo build --bin qm
\`\`\`

## Three commands to a verdict

\`\`\`
qm doctor   # is any backend runnable? (reachable + has a model + creds OK)
qm init     # auto-detect a backend, write ./qm.json, run a first verdict
qm run      # the readiness verdict — Ready / Conditional / NotReady
\`\`\`

In a terminal, omitting \`--model\` / \`--collection\` / \`--backend\` opens interactive pickers; over
SSH or CI it never prompts.

## The commands

| Command | The app equivalent |
| --- | --- |
| \`qm doctor\` | The backend health check — "is anything runnable?" |
| \`qm init\` | Zero-config first run (writes \`qm.json\`; later runs need no flags) |
| \`qm run\` | Tests → Run Batch on a **built-in** collection |
| \`qm test\` | Tests → Run Batch on **your own** collection or an **MCP world** file |
| \`qm validate\` | The import validator — prove a collection/world before trusting its score |
| \`qm cliff\` | The Audit tab's **Context Stress Test** |
| \`qm report\` | The Agent Report verdict, re-scored from a saved run (no backend, no re-inference) |
| \`qm prompt\` | Workspace free-form generation |

Run \`qm COMMAND --help\` for any command's full flag reference.

## Exit codes gate CI

The verdict *is* the exit status: **0** Ready · **10** Conditional · **20** NotReady · **11**
Inconclusive (infra fault — retry, never a fake "bad model") · **3** nothing runnable. \`--fail-on\` is
the team-policy knob, and a bundled GitHub Action (\`.github/actions/qm-eval\`) wires it into a pipeline
with a JUnit report.

> [!IMPORTANT]
> Secrets discipline: never pass an API key as an argument. \`qm\` reads \`QM_API_KEY\` from the
> environment (or the OS keychain) only; a key sent over plain \`http\` is withheld with a warning.

## Next steps

- **Troubleshooting** — common issues and fixes.
`.trim();

const troubleshooting = `
# Troubleshooting

Common issues and quick fixes.

## Run Batch does nothing / returns n=0

Almost always: **native tool-calling** is selected but the model can't do it. Tick **Prompt-based**,
or use a tools-capable model (e.g. \`qwen3.5\`). A run that can't start now shows the reason beside
the button.

## "does not support tools"

Your model advertises only \`completion\`, not \`tools\` — typical for hand-imported quants. Use
Prompt-based tool-calling, or pick a model built with a tool template.

## Backend dot is grey / red

The selected backend's server isn't reachable. Start it:

- **Ollama** — make sure the Ollama app/service is running.
- **llama.cpp** — re-select the GGUF model to relaunch the bundled server.
- **MLX** — Apple Silicon only; check the model folder exists.

## Out of memory / "Compute error"

The model + KV cache exceeded your GPU/unified memory. Use a smaller quant, a smaller context
window, or a smaller model. On llama.cpp, lower the context size.

## Context overflow

Your \`num_ctx\` is larger than the model or your memory supports. Lower the context window in the
run settings, or check the **Latency** context-ceiling bars for a safe value.

## KV-cache ceiling says "Not available"

Load the model in the **selected** backend first — the bars need the model's dimensions and its
loaded size. Make sure the header backend matches the model you're inspecting.

## MLX not detected

MLX only runs on Apple Silicon. On Intel Macs or other platforms it isn't offered.

## MCP server won't connect ("✗ …")

The connect probe launches your **command + args** as a local process, so the usual cause is a
missing runtime or a typo. MCP servers here run through **npx**, so **Node.js** must be installed
(\`node -v\`). Re-check the command and args; the red error names what failed. A server that prints
anything but JSON-RPC to stdout also fails the probe — that's deliberate, so it fails at setup rather
than corrupting a run.

## MCP world validation fails / "do-nothing agent passed"

Your answer key isn't *discriminating* — an empty run satisfies it, so it can't tell a working model
from a broken one. Tighten the oracle (assert a file's **content**, not just its presence; make the
DB assertion specific). The builder and \`qm validate\` both reject this on purpose: a broken answer
key makes every Pass^k meaningless.

## MCP world "Inconclusive" instead of a verdict

A live world needs **npx** (any world) and **sqlite3** (database worlds). When they're missing the
run is honestly inconclusive — never a fake failure — and the message prints the exact install
command. Install the dependency and re-run.

## Still stuck?

The **Reference** section (in the sidebar) has a per-feature breakdown of what every page does and
why, and the app's status messages usually name the exact fix.
`.trim();

export const DOC_SECTIONS: DocSection[] = [
  {
    id: "get-started",
    title: "Get started",
    pages: [
      { id: "getting-started", title: "Getting started", description: "What QuantaMind is and how to begin.", body: gettingStarted },
      { id: "choosing-a-backend", title: "Choosing a backend", description: "Ollama, llama.cpp, or MLX — and native tool-calling.", body: choosingBackend },
    ],
  },
  {
    id: "guides",
    title: "Guides",
    pages: [
      { id: "adding-models", title: "Adding models", description: "Install and download models per backend.", body: addingModels },
      { id: "workspace", title: "Working in the Workspace", description: "Chat with a model and read its live run metrics.", body: workspace },
      { id: "analysis", title: "Comparing models", description: "Run one prompt across several models, side by side.", body: analysis },
      { id: "first-test", title: "Running your first test", description: "Run and read an evaluation.", body: firstTest },
      { id: "custom-collections", title: "Building your own test collection", description: "Author tasks by hand or import a CSV.", body: customCollections },
      { id: "agent-report", title: "Reading the Agent Report", description: "Turn scores into a readiness verdict.", body: agentReport },
      { id: "latency", title: "Latency & memory of a Test run", description: "What a test run cost: per-task speed, cache, KV memory, and context ceilings.", body: latency },
    ],
  },
  {
    id: "advanced",
    title: "Advanced",
    pages: [
      { id: "mcp", title: "Testing with MCP servers", description: "Grade a model against real tools and real world-state.", body: mcp },
      { id: "cli", title: "The qm command-line tool", description: "The headless engine — run and gate CI on the same verdicts.", body: cli },
    ],
  },
  {
    id: "troubleshooting",
    title: "Troubleshooting",
    pages: [
      { id: "troubleshooting", title: "Troubleshooting", description: "Common issues and quick fixes.", body: troubleshooting },
    ],
  },
  referenceToDocSection(),
];

export const DEFAULT_PAGE_ID = "getting-started";

export type LocatedPage = { section: DocSection; page: DocPage };

/// Flattened list of every page with its section — for search + lookup.
export const ALL_PAGES: LocatedPage[] = DOC_SECTIONS.flatMap((section) => section.pages.map((page) => ({ section, page })));

export function findPage(id: string): LocatedPage | null {
  return ALL_PAGES.find((p) => p.page.id === id) ?? null;
}

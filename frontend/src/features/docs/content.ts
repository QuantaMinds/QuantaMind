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
# Latency & context

The **Latency** tab (also called the Inspector) shows how *fast* a model runs and how much *context*
it can hold on your machine.

## Token timing

For each charted run you get time-to-first-token, per-token latency, and a histogram — so you can
see not just the average speed but its consistency.

## VRAM / unified memory

The **VRAM bar** shows the loaded model's footprint against your device's memory
("6.6 GB in unified memory of 16.0 GB"). It needs the model actually loaded in the running server;
if it isn't, you'll see "load the model to measure".

## Context ceiling by KV-cache precision

This estimates how much **context** fits in memory at three KV-cache precisions:

- **f16** — full precision (the default).
- **q8** — roughly 2× the context at negligible quality cost.
- **q4** — roughly 4× the context, but with a real quality cost and often slower at long context.

> [!NOTE]
> These bars need the model's dimensions. For **llama.cpp** they come from the GGUF header (with the
> model loaded); for **Ollama** from the model's reported metadata. If a bar says "Not available",
> load the model in the selected backend first.

## Next steps

- **Troubleshooting** — if something's not measuring or running.
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
      { id: "first-test", title: "Running your first test", description: "Run and read an evaluation.", body: firstTest },
      { id: "agent-report", title: "Reading the Agent Report", description: "Turn scores into a readiness verdict.", body: agentReport },
      { id: "latency", title: "Latency & context", description: "Speed, VRAM, and context ceilings.", body: latency },
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

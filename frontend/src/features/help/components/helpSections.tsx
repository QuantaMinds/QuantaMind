import type { ReactNode } from "react";

/// One documented thing — a panel, a graph, a control, or a computed metric.
/// Every block answers the same three questions, so the page reads consistently
/// no matter which feature it describes. `formula`/`source` are for blocks whose
/// number is computed (the user can see exactly how it's derived and where).
export interface HelpBlock {
  /// Anchor within a section (url hash → `#help-<section>-<id>`).
  id: string;
  heading: string;
  what: ReactNode;
  why: ReactNode;
  how: ReactNode;
  /// A literal formula / algorithm, rendered monospace. Optional.
  formula?: string;
  /// Where the number is actually computed, e.g. a Rust file. Optional.
  source?: string;
}

/// One page of the app (or a cross-cutting topic), shown as a sidebar entry with
/// its blocks in the center pane.
export interface HelpSection {
  /// Sidebar id + url hash (`#help-<id>`).
  id: string;
  title: string;
  /// One-line summary under the section title.
  blurb: string;
  blocks: HelpBlock[];
}

export const HELP_SECTIONS: HelpSection[] = [
  {
    id: "workspace",
    title: "Workspace",
    blurb: "Your home base — pick a model, write a prompt, run it, watch tokens stream.",
    blocks: [
      {
        id: "first-run",
        heading: "First run — getting Ollama up",
        what: "On a fresh machine the model dropdown is replaced by an “Ollama is not running” card with Start Ollama and Install Ollama buttons.",
        why: "QuantaMind doesn’t ship its own model runtime — it drives Ollama, a local server, so your models and weights stay on your machine and nothing is sent to the cloud.",
        how: "“Start Ollama” launches the server in the background (it keeps running after you quit QuantaMind). “Install Ollama” opens ollama.com/download. Once it’s up, the Models tab lets you pull a model — try llama3.2:1b (~700 MB) for a quick first run.",
      },
      {
        id: "model-select",
        heading: "Model picker + temperature + stop",
        what: "The bar at the top: a model dropdown, a gear (⚙) on its left, and a square stop icon on its right.",
        why: "Everything the app does runs against the globally-selected model, so this selection is the one knob the whole app reads — there’s no per-page model choice to keep in sync.",
        how: "The gear opens a temperature popover (0.0–2.0, persisted per model). The stop icon kills the Ollama server — the health dot in the status bar flips to red immediately. For Ollama you can multi-select 2+ models, which turns the run into a Compare (see Analysis).",
      },
      {
        id: "prompts",
        heading: "System + user prompt, Run, and streaming",
        what: "Two editors — an optional System prompt and the User prompt — plus a Run button. Output streams into the panel below.",
        why: "Separating system from user prompt mirrors how the model is actually called, so what you test here matches what your app will send in production.",
        how: "Run streams tokens as they’re generated. A large model shows “Loading model…” for up to ~30s before the first token (the weights load into RAM/VRAM). Cancel mid-stream stops generation cleanly and keeps whatever was produced.",
      },
      {
        id: "run-metrics",
        heading: "Run metrics (TTFT · tokens/s · token count)",
        what: "After a run the metrics row shows time-to-first-token, throughput, and total tokens.",
        why: "These three numbers are the headline of local-model performance: how long until it starts, how fast it sustains, and how much it produced. They’re measured, never estimated.",
        how: "TTFT is the wall-clock from request to the first streamed token. Throughput divides the generated-token count by the span from the first token to the last (the steady-state rate, excluding the initial wait). Token counts come from the backend’s own counters, not a chars÷4 guess.",
        formula: "ttft_ms      = first_token_time − request_start\ntokens_per_sec = token_count ÷ (last_token_time − first_token_time)",
        source: "backend/src/metrics/timing.rs · backend/src/metrics/throughput.rs",
      },
      {
        id: "templates",
        heading: "Prompt template picker",
        what: "A picker that inserts a ready-made prompt skeleton into the User prompt box.",
        why: "Common tasks (summarize, extract JSON, tool-call) have a known-good prompt shape; starting from one saves you re-deriving it and makes runs comparable.",
        how: "Templates are bundled app assets under docs/prompts/; selecting one pastes its text at the cursor — you then edit freely.",
      },
    ],
  },

  {
    id: "analysis",
    title: "Analysis",
    blurb: "Run one prompt across several models and compare their output and speed side by side.",
    blocks: [
      {
        id: "compare-columns",
        heading: "Compare columns",
        what: "One column per selected model, each streaming its own raw output independently.",
        why: "The only honest way to choose between models for a prompt is to see them answer the same prompt under the same settings, at the same time.",
        how: "Select 2+ models in the Workspace picker (Ollama), run, and each model gets a column that streams as its tokens arrive. Columns are independent — one being slow doesn’t hold up the others.",
      },
      {
        id: "strategies",
        heading: "Run strategies (sequential · skippable · parallel)",
        what: "A picker that controls how multiple models share your machine during a compare run.",
        why: "RAM is the constraint with local models; the right strategy depends on whether you’d rather protect memory or wall-clock time.",
        how: "Sequential loads one model at a time and sends keep_alive=0 so each is evicted before the next loads (best on limited RAM). Sequential (skippable) is the same but adds a Skip button per row. Parallel loads all models at once — fastest if you have the RAM, each row showing its own spinner until first token.",
      },
      {
        id: "metrics-chart",
        heading: "Metrics chart (throughput + TTFT bars)",
        what: "Two bar charts comparing the models’ tokens/sec and time-to-first-token.",
        why: "Output quality is subjective; speed is not. Charting throughput and TTFT side by side makes the speed trade-off between models obvious at a glance.",
        how: "Bars are drawn from the same measured per-run metrics shown in the Workspace status bar (tokens_per_sec, ttft). A metric the backend didn’t measure stays null and is drawn as absent — never as a fake 0. Axis ticks are derived from the data’s own range, not a hardcoded ceiling.",
        formula: "throughput bar = metrics.tokens_per_sec   (null ⇒ no bar)\nTTFT bar       = metrics.ttft_ms",
        source: "frontend/src/features/compare/components/MetricsChart.tsx",
      },
      {
        id: "compare-diff",
        heading: "Output diff",
        what: "A word-level diff between two models’ outputs.",
        why: "When two answers look similar, the interesting signal is exactly where they diverge — a diff surfaces that without you hunting line by line.",
        how: "Pick two columns; added/removed words are highlighted inline so you can read the substantive differences directly.",
      },
      {
        id: "export",
        heading: "Export (Markdown / JSON)",
        what: "Buttons to save the whole comparison run.",
        why: "Results are worth keeping — for a write-up, a ticket, or a later regression check against the Audit timeline.",
        how: "Export Markdown for a human-readable record or JSON for the machine-readable analysis document (its schema is the bench/analysis contract in docs/reference.md).",
      },
    ],
  },

  {
    id: "inspector",
    title: "Latency",
    blurb: "Per-token timing forensics for a run — exactly where the milliseconds go.",
    blocks: [
      {
        id: "ttft-breakdown",
        heading: "TTFT breakdown (stacked phase bar)",
        what: "A horizontal stacked bar splitting the time-to-first-token into model-load, prompt-prefill, and generation phases.",
        why: "A high TTFT can mean two very different things — a cold model loading from disk, or a huge prompt being prefilled. Splitting the phases tells you which one to fix.",
        how: "Each segment is the measured duration of that phase from the run’s timing record; the widths are proportional to real elapsed time.",
        source: "frontend/src/features/inspector/components/TtftBreakdown.tsx",
      },
      {
        id: "token-timeline",
        heading: "Token timeline (per-token latency)",
        what: "A chart (visx) with one bar per generated token: x = time since start, y = the gap since the previous token.",
        why: "Average tokens/sec hides stalls. The timeline shows the actual rhythm — and any mid-stream pauses where a token took far longer than its neighbours.",
        how: "Built from the backend’s per-token timeline (each token’s cumulative t_ms). The first token (TTFT) is amber, normal gaps blue, and statistical outliers red; dashed lines mark the load/prefill/generation boundaries.",
        formula: "gapₙ = t_msₙ − t_msₙ₋₁   (per-token inter-arrival time)",
        source: "backend/src/metrics/timeline.rs · frontend/.../inspector/components/TokenTimeline.tsx",
      },
      {
        id: "latency-histogram",
        heading: "Latency histogram",
        what: "A distribution chart (visx) bucketing inter-token gaps into latency bins with a token count per bin.",
        why: "It answers “is this model consistently fast, or fast-on-average with a long tail?” — the tail is what users actually feel.",
        how: "Inter-token gaps from the timeline are bucketed into ranges (e.g. 0–10ms, 10–20ms); normal bins are blue, outlier bins rose. Hover a bar for its range and count.",
        source: "frontend/src/features/inspector/components/LatencyHistogram.tsx",
      },
      {
        id: "vram-bar",
        heading: "VRAM bar + leak banner",
        what: "A bar of the run’s memory footprint, plus a banner if a VRAM leak is detected across runs.",
        why: "Local inference lives or dies by memory; a creeping allocation that never frees will eventually push a model into slow partial-offload or an outright failure.",
        how: "The bar shows measured allocation; the leak banner fires when memory after a run stays elevated instead of returning to baseline.",
      },
      {
        id: "context-budget",
        heading: "Context budget bar",
        what: "A bar showing how much of the model’s context window the prompt consumed.",
        why: "Running close to the context limit is where models start dropping earlier instructions — this is the early-warning gauge for that.",
        how: "Divides the backend’s real prompt-token count for the run by the model’s context length (from GGUF metadata). It reads the live cliff store, not a stale cache.",
        formula: "context_used = prompt_tokens ÷ context_length",
        source: "frontend/src/features/inspector/components/ContextBudgetBar.tsx",
      },
      {
        id: "regression-export",
        heading: "Regression alert + report export",
        what: "A banner when this run is slower than recent runs, plus a button to export the full timing report.",
        why: "Performance regressions (a model update, a settings change) are easy to miss run-to-run; flagging them automatically and letting you export keeps a record.",
        how: "Compares the run’s key timings against recent history; export writes the per-token timeline and phase breakdown out for sharing.",
      },
    ],
  },

  {
    id: "models",
    title: "Models",
    blurb: "Install models three ways — this is where weights come from.",
    blocks: [
      {
        id: "ollama-library",
        heading: "Ollama Library tab",
        what: "Search any tag from ollama.com/library and install it with a streaming progress bar.",
        why: "The fastest path to a working model — the curated registry most local-agent builders already use.",
        how: "Type a tag (the description links to the library), click Install, and the pull streams progress into the Downloads tab.",
      },
      {
        id: "huggingface",
        heading: "Hugging Face tab",
        what: "Search a GGUF repo, pick a specific quantization from the file list, and install.",
        why: "Hugging Face has a far wider selection (and more quant choices) than the Ollama library — useful when you want a specific quant of a specific model.",
        how: "QuantaMind downloads the chosen GGUF, generates the right Modelfile (chat template + params), and registers it with Ollama so it shows up like any other model.",
      },
      {
        id: "local-file",
        heading: "Local File import",
        what: "Point at a .gguf already on disk; QuantaMind registers it with Ollama — no re-download.",
        why: "If you’ve already downloaded weights (or built your own), re-fetching them would be wasteful and slow.",
        how: "QuantaMind parses the GGUF v3 header to read the model’s real dimensions, generates a matching Modelfile, and creates the Ollama entry in place.",
        source: "backend GGUF v3 header parser (Models → local import)",
      },
    ],
  },

  {
    id: "downloads",
    title: "Downloads",
    blurb: "Every in-progress install, with live progress and the current phase.",
    blocks: [
      {
        id: "downloads-list",
        heading: "Active downloads",
        what: "A list of every active pull / HF download / local install with a progress bar and a phase label.",
        why: "Installs are long-running and multi-stage; a single place to watch them (and cancel) keeps the other tabs uncluttered.",
        how: "Each row shows its phase — downloading, verifying, writing, success — streamed from the backend. Cancel stops it cleanly; partial files are kept and resumed if you re-install with the same name. A finished download disappears here and the model appears in the Workspace dropdown and Storage.",
      },
    ],
  },

  {
    id: "eval",
    title: "Tests",
    blurb: "Score models on tool-calling and multi-step agentic tasks — the engine behind the scoreboard.",
    blocks: [
      {
        id: "eval-manager",
        heading: "Tests Manager",
        what: "The left panel: a collection picker, a target-model multi-select, Iterations (k) and Max Steps inputs, and Run/Stop.",
        why: "An eval is (which tasks) × (which models) × (how many repeats). The manager is where you set those three before a batch runs.",
        how: "Pick a built-in or custom collection, choose target models, set k and the step cap, then Run. Switching collection or model mid-batch cancels cleanly and clears stale results so a leftover Pass/Fail never bleeds into the new run.",
      },
      {
        id: "iterations",
        heading: "Iterations (k) — the k in Pass^k",
        what: "How many times each multi-step (agentic) task is re-run.",
        why: "A real agent loops over many steps where small failures compound, so passing once isn’t evidence of reliability — you need it to pass repeatedly.",
        how: "With k=5 a task runs 5× and the model is only “reliable” if it passes consistently (reported as passes/total, e.g. 4/5). Higher k = a stricter bar and longer runs; k=1 = no consistency check. Single-turn tool-call tasks always run once (k doesn’t apply).",
      },
      {
        id: "csv-import",
        heading: "Collection editor + CSV import",
        what: "Author custom task collections by hand, or bulk-load single-turn tool-call tasks from a spreadsheet.",
        why: "Your tasks — not a generic fixture — are what tell you whether a model is ready for your app. Custom collections are first-class here.",
        how: "The CSV needs exactly four columns in order: id, prompt, expected_tool, expected_args (args as a JSON object). One row = one task; leave expected_tool empty for an abstain task (correct behavior = no call). Tool schemas live in a separate box and apply to every row. The dialog validates live — wrong header order, bad args JSON, an unknown tool, or a duplicate id are flagged per-row and Import stays disabled until clean.",
      },
      {
        id: "simulator",
        heading: "Simulator grid",
        what: "A grid of pass/fail cells, one per (model × task), filling in live as the batch runs.",
        why: "The aggregate scores tell you who won; the grid tells you which specific tasks each model passed or failed — where to look next.",
        how: "Each cell turns green (pass), red (fail), or stays grey (pending) as results stream in. Click through to the trace debugger to see exactly what happened on one task.",
      },
      {
        id: "toolcall-accuracy",
        heading: "Metric — tool-call composite accuracy",
        what: "The 0–100% score for single-turn tool-calling tasks (the “Accuracy” you see in the cliff probe and the matrix).",
        why: "“Did it call the right tool, the right way?” is several different questions (did it emit a call at all, the right tool, the right args, and stay silent when it shouldn’t call). One blended number with honest sub-parts captures all of them.",
        how: "Four sub-metrics with cascaded conditional denominators — so a format failure never bleeds into the reasoning metrics — are averaged. A sub-metric with a zero denominator is n/a (excluded), not 0. Scoring is structural (match name + args), never by executing the tool.",
        formula:
          "parse_rate         = parsed calls       ÷ tasks expecting a call\n" +
          "tool_selection_acc = right tool name   ÷ parsed calls\n" +
          "arg_acc            = right arguments   ÷ tool-matched tasks\n" +
          "abstain_acc        = correct no-calls  ÷ tasks expecting no call\n" +
          "composite          = mean(of the sub-metrics that applied)",
        source: "backend/src/inference/eval/toolcall/eval.rs (aggregate)",
      },
      {
        id: "passk",
        heading: "Metric — Pass^k",
        what: "Reliability across repeats: how many of the k runs of an agentic task reached the end state.",
        why: "It’s the headline reliability number — a model that succeeds 5/5 is trustworthy in a loop; 3/5 is a coin-flip you can’t ship.",
        how: "A run “passes” only if it actually reaches the task’s end state (not if it merely claims to be done). Pass^k is passes over total runs, shown as passes/total.",
        formula: "pass^k = passes ÷ total_runs   (passes = runs where reached_end == true)",
        source: "backend/src/inference/eval/agentic/report.rs (from_outcomes)",
      },
      {
        id: "avg-steps",
        heading: "Metric — Avg Steps",
        what: "The mean number of agentic steps (tool calls) a model took, across all runs.",
        why: "Two models can both pass, but the one that solves it in 3 steps instead of 8 is cheaper and less likely to wander — efficiency matters as much as success.",
        how: "Mean of every run’s step count, including failed runs (a model that loops to the step cap should pay for it in this number). Single-turn tasks have no steps and show “—”.",
        formula: "avg_steps = mean(steps over ALL runs)   (None ⇒ “—”)",
        source: "backend/src/inference/eval/agentic/report.rs (mean)",
      },
      {
        id: "effort",
        heading: "Metric — Effort",
        what: "Mean output tokens spent on the runs that succeeded.",
        why: "The token cost of getting it right. Counting failed runs would reward a model for giving up early, so they’re deliberately excluded.",
        how: "Mean of output_tokens (eval_count — output only, prompt tokens are never summed) over successful runs only. No success ⇒ N/A, never a fabricated 0.",
        formula: "effort = mean(output_tokens over runs where reached_end == true)   (None ⇒ N/A)",
        source: "backend/src/inference/eval/agentic/report.rs (from_outcomes)",
      },
      {
        id: "schema-resilience",
        heading: "Metric — Schema Resilience",
        what: "Of the runs that hit a schema error, the share that recovered with a valid call.",
        why: "Models will sometimes emit a call with a missing or mistyped argument; what matters for an agent is whether it can recover after the correction is injected.",
        how: "Counts runs that hit a schema-invalid call, and of those, how many later produced a valid one. If no run ever hit a schema error the metric didn’t apply and shows “—” (never 0).",
        formula: "schema_resilience = recovered_runs ÷ runs_that_hit_a_schema_error   (None ⇒ “—”)",
        source: "backend/src/inference/eval/agentic/report.rs (from_outcomes)",
      },
      {
        id: "top-error",
        heading: "Metric — Top Error",
        what: "The dominant failure mode in a model’s row: Loop Cap, Fake Done, Bad Schema, or Malformed.",
        why: "When a model fails, the kind of failure tells you what to fix — a looping model, a model that lies about finishing, and one that can’t emit valid JSON need different responses.",
        how: "The four failure tallies never overlap; Top Error is the argmax. Ties break by severity: infinite-loop > hallucinated > bad-schema > malformed-json. “None” means no failures. The ⓘ beside the badge lists all four counts.",
        formula: "top_error = argmax(loop, fake_done, bad_schema, malformed)\n           ties → severity: loop > fake_done > bad_schema > malformed",
        source: "backend/src/inference/eval/agentic/report.rs (FailureTracker::top)",
      },
      {
        id: "performance-matrix",
        heading: "Model Results",
        what: "The per-model summary table: Pass^k · Avg Steps · Effort · Schema Resil. · Context Limit · Top Error (+ an optional Native-FC column).",
        why: "One row per model is the at-a-glance verdict surface — every metric above, lined up so models are directly comparable.",
        how: "Each cell renders the corresponding measured metric (see the metric blocks above). N/A and “—” states are explained on hover; click a row to inspect that model. An always-visible legend explains the Context Limit column.",
      },
      {
        id: "context-cliff",
        heading: "Context Stress Test + chart",
        what: "Runs a dataset at growing prompt lengths and graphs (visx) where tool-call accuracy collapses — the point where long context breaks tool use.",
        why: "Many local models break down well before their advertised context window. The probe finds the real usable window for tool use, which feeds the Agent-Readiness verdict.",
        how: "The x-axis is the model’s real measured prompt-token depth (prompt_eval_count, averaged per rung) — never a chars÷4 estimate; the y-axis is the composite accuracy above. The verdict is computed so the persisted depth and the badge can never disagree: a healthy baseline (rung 0 ≥ 50%) that then drops ≥20pp = a cliff at that rung’s depth; a healthy baseline that holds = “✓ no cliff”; a baseline already below 50% = “fails from start” (broken at the smallest context, a tool-call failure — not a context limit); an errored baseline = unmeasured. Because a cliff is a diagnostic, the probe defaults to Greedy (temperature 0) decoding so the same (model, collection) reproduces the same verdict run-to-run — untick “Greedy (temp 0)” to sample at your global temperature instead. The probe never auto-runs: you start it with Execute Probe, or from the Model Results table via “Run probe ↗” on an un-measured model or the “↻” re-probe control beside an already-measured badge — both pre-fill the model + collection and open the Audit tab.",
        formula:
          "baseline = composite(rung 0)\n" +
          "if baseline < 0.50            → broken-baseline (“fails from start”)\n" +
          "elif any rung ≤ baseline−0.20 → cliff at that rung’s prompt_tokens\n" +
          "else                          → no-cliff (“✓ no cliff”)",
        source: "frontend/src/features/eval/cliff.ts (classifyCliff)",
      },
      {
        id: "trace-debugger",
        heading: "Trace debugger (pipeline visualizer)",
        what: "Drill into a single task: its config, the system message built for it, the raw model output, and the verdict — phase by phase.",
        why: "A red cell isn’t actionable until you can see what the model actually produced and why it was marked wrong. This is that view, without re-running inference.",
        how: "Each task persists its full trace; the visualizer replays the phases (Config → System message → Stream → Verify) from that saved trace.",
      },
    ],
  },

  {
    id: "audit",
    title: "Audit",
    blurb: "Regression history over time, plus the saved record of past batch runs.",
    blocks: [
      {
        id: "history-timeline",
        heading: "History timeline",
        what: "A line chart (one series per model) of each model’s score across consecutive runs.",
        why: "Single runs can’t tell you about regressions; a trend line catches the moment a model — or a prompt change — starts doing worse.",
        how: "X = run order (oldest → newest), Y = composite/pass-rate %. Each model is a coloured line with dots; hover a dot for its run number and exact value. It tracks the same metrics the Model Results table reports.",
        source: "frontend/src/features/eval/components/matrix/HistoryTimeline.tsx",
      },
      {
        id: "cliff-here",
        heading: "Context Stress Test (also here)",
        what: "The same Context Stress Test and chart from the Tests tab, reachable from Audit.",
        why: "The context limit is part of the audit story for a model, so the test lives where you review history too — and “Run probe ↗” from Model Results pre-fills and lands here.",
        how: "Identical to the test documented under Tests → Context Stress Test (verdicts, the 50% baseline gate, the ≥20pp collapse rule). On completion the depth is saved to the backend per (collection, model).",
      },
      {
        id: "audit-export",
        heading: "CSV export",
        what: "Export the audit trail of runs as CSV.",
        why: "For sharing a regression record or pulling the numbers into a spreadsheet/report.",
        how: "Writes the per-run scores behind the timeline out as a flat CSV.",
      },
    ],
  },

  {
    id: "quant",
    title: "Quant",
    blurb: "Compare quantizations of one model family — size vs quality vs whether it fits.",
    blocks: [
      {
        id: "selectors",
        heading: "Family / use-case / context-length selectors",
        what: "Pick a model family (one with 2+ installed quants), a use case, and a target context length.",
        why: "“Which quant should I run?” has no universal answer — it depends on what you’re doing and how much memory you can spare. These three inputs frame the comparison.",
        how: "The family picker only lists families with multiple quants to compare; the use case and context length drive the Fit and Recommendation columns.",
      },
      {
        id: "quant-table",
        heading: "Quant comparison table",
        what: "A row per quantization with Size, Fit, Quality (eval pass-rate), Tool-calls (composite %), and a Recommendation badge.",
        why: "Lower quants are smaller and faster but lossy; this table makes the size↔quality↔fit trade-off explicit so you pick deliberately, not by guesswork.",
        how: "Size is on-disk bytes. Quality and Tool-calls are filled by running the same evals documented under Eval (pass-rate and tool-call composite). The Recommendation marks the best trade-off for your selected use case, with a delta vs the best quant.",
      },
      {
        id: "fit",
        heading: "Fit (OOM risk)",
        what: "A safe / tight / won’t-fit indicator per quant at your chosen context length.",
        why: "A quant that won’t fit in memory either fails or falls back to painfully slow partial offload — you want to know before you load it.",
        how: "Sums the exact on-disk weight bytes and the real KV-cache bytes for the chosen context length and compares to the memory cap (same math as the Agent Report’s VRAM fit). Approximate inputs are flagged with a ~.",
        formula: "total = weights_bytes + kv_cache_bytes(context)\nfit:  total ≤ cap → safe · ≥ 85% of cap → tight · > cap → won’t fit",
        source: "backend/src/inference/eval/readiness/vram_fit.rs",
      },
    ],
  },

  {
    id: "agentReport",
    title: "Agent Report",
    blurb: "Turn the measurements into a go / no-go readiness verdict per model.",
    blocks: [
      {
        id: "hardware-profile",
        heading: "Host hardware profile + VRAM cap",
        what: "Your detected CPU/RAM/GPU and a slider that caps how much memory a model may use.",
        why: "Readiness is relative to your machine. The cap lets you ask “would this be ready on a box with N GB?” without owning that box.",
        how: "Hardware is detected at runtime; the cap feeds the VRAM-fit gate below. Lowering it makes the fit test stricter.",
      },
      {
        id: "profiles",
        heading: "Readiness profiles",
        what: "Named requirement sets (e.g. Coding Agent, RAG Assistant, General Agent) you can pick and edit.",
        why: "“Ready” means nothing without “ready for what”. A coding agent needs context headroom and no loops; a quick assistant may tolerate more. The profile encodes those thresholds.",
        how: "A profile sets the hard gates (min Pass^k, forbid loops/fake-done, require full VRAM, min context tokens, require native FC) and soft targets (max ms/step, max avg steps). Edit them in the profile modal.",
      },
      {
        id: "verdict-logic",
        heading: "How the verdict is decided",
        what: "Each model gets 🟢 Ready, 🟡 Conditional, or 🔴 Not Ready, with the exact blocking and conditional reasons listed.",
        why: "A single badge is only trustworthy if you can see why — so the verdict always shows the reasons, and a required-but-unmeasured metric blocks rather than passing silently (ignorance is not a pass).",
        how: "Hard gates that fail → blocking reasons → Not Ready. Soft targets that breach → conditions → Conditional. All clear → Ready. Unmeasured behaves differently by gate type: a required hard gate blocks when unmeasured; a soft target stays silent when unmeasured.",
        formula:
          "if blocking not empty   → Not Ready\n" +
          "elif conditions not empty → Conditional\n" +
          "else                    → Ready\n" +
          "hard gates: pass^k ≥ min · no loops · no fake-done · VRAM fits · cliff ≥ min_context · native-FC (if required)\n" +
          "soft targets: ms/step ≤ max · avg_steps ≤ max",
        source: "backend/src/inference/eval/readiness/verdict.rs (assess)",
      },
      {
        id: "vram-fit",
        heading: "VRAM fit (memory profile)",
        what: "Per model: exact weights + KV cache at the run’s context length, vs the cap, with a pressure flag.",
        why: "Partial offload is the silent killer of local-agent latency; the readiness verdict needs a truthful fit test, not a guess.",
        how: "Weights are the exact on-disk bytes (never estimated); the KV cache uses the canonical f16 formula from the model’s real dims at the run’s context length. Fits = total ≤ cap; pressure = fits but ≥ 85% of the cap (a soft Conditional note). Any missing input ⇒ “not measured” (the verdict treats VRAM as unmeasured, never a guessed fit).",
        formula: "total = weights_bytes + kv_cache_bytes\nfits = total ≤ cap · pressure = total ≥ 0.85 × cap",
        source: "backend/src/inference/eval/readiness/vram_fit.rs (estimate)",
      },
      {
        id: "native-fc",
        heading: "Native function-calling path",
        what: "A label (and optional column) showing whether readiness was judged via the model’s native tool_calls API or the prompt-based proxy.",
        why: "Native function-calling and prompt-based tool-calling are different reliability stories; the verdict is honest about which one it measured.",
        how: "If native FC was tested (Ollama /api/chat tools), the path is Native-FC and its Pass^k is preferred; otherwise Prompt-Based. A profile can require native FC as a hard gate.",
        source: "backend/src/inference/eval/readiness/verdict.rs · types.rs",
      },
      {
        id: "report-export",
        heading: "Export the readiness report (Image · Markdown · HTML)",
        what: "The Export Report menu offers three fully-offline formats: a PNG image of the report card, the report as GitHub-flavoured Markdown copied to your clipboard, and a standalone HTML file.",
        why: "The readiness call is something you share with a team, paste into a ticket, or attach to a decision — and different destinations want different forms. None of them needs auth, a network, or the community board.",
        how: "“Export as Image (.png)” rasterizes the live report card (a thin Rust sink writes the bytes to a path you pick). “Copy Markdown” builds a GFM table plus per-model reasons and puts it on the clipboard. “Export HTML” downloads a self-contained file. Every format renders the same measured verdicts — an unmeasured metric is written as “N/A”, never fabricated, and HTML content is escaped, never injected as raw markup.",
        source: "frontend/src/features/agentReport/components/ExportMenu.tsx · export/markdown.ts · reportHtml.ts",
      },
      {
        id: "publish-board",
        heading: "Publish to Board (opt-in community share)",
        what: "A “Publish to Board” button that contributes your aggregate readiness numbers to the community leaderboard — behind an explicit, default-off privacy gate.",
        why: "Cross-machine readiness data is what makes the recommender useful, but it can never come at the cost of leaking your tasks. So sharing is opt-in, aggregate-only, and shows you the exact bytes before any leave the machine.",
        how: "The dialog shows a Shared / Never-shared panel and the literal canonical JSON payload. It shares only metrics (Pass^k, effort, avg steps), hardware cohort tags, model name + quant, and an integrity hash — never task content, prompts, file names, or raw traces. Publish stays disabled until you tick the opt-in box (it starts unchecked); an optional write-up link is allow-listed to a few domains. The app is 100% functional offline — every server outcome (sign-in needed, rate-limited, rejected row) becomes a toast and never freezes the UI. Results are labelled “community-reported”.",
        source: "frontend/src/features/publish/PublishDialog.tsx · WhatsSharedPanel.tsx",
      },
    ],
  },

  {
    id: "settings",
    title: "Settings",
    blurb: "Detected hardware and app-level information.",
    blocks: [
      {
        id: "hardware-section",
        heading: "Hardware section",
        what: "A read-out of detected hardware (CPU, RAM, GPU) and VRAM status.",
        why: "Several features (Quant fit, Agent Report VRAM gate) reason about your machine; this is the one place to see what the app detected.",
        how: "Hardware is probed at runtime and shown here; the same detection feeds the readiness and fit calculations.",
      },
    ],
  },
];

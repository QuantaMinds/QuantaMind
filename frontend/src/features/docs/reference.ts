import type { DocPage, DocSection } from "./content";

/// One documented thing — a panel, a graph, a control, or a computed metric.
/// Every block answers the same three questions, so the page reads consistently
/// no matter which feature it describes. `formula`/`source` are for blocks whose
/// number is computed (the user can see exactly how it's derived and where).
export interface ReferenceBlock {
  /// Anchor within a page — the block heading's slug is the `#docs-…--<slug>` target.
  id: string;
  heading: string;
  what: string;
  why: string;
  how: string;
  /// A literal formula / algorithm, rendered monospace. Optional.
  formula?: string;
  /// Where the number is actually computed, e.g. a Rust file. Optional.
  source?: string;
}

/// One page of the app (or a cross-cutting topic), shown as a Reference sidebar entry.
export interface ReferenceSection {
  id: string;
  title: string;
  /// One-line summary under the section title.
  blurb: string;
  blocks: ReferenceBlock[];
}

export const REFERENCE_SECTIONS: ReferenceSection[] = [
  {
    id: "workspace",
    title: "Workspace",
    blurb: "Your home base — pick a model, write a prompt, run it, watch tokens stream.",
    blocks: [
      {
        id: "first-run",
        heading: "First run — getting Ollama up",
        what: "On a fresh machine the model dropdown is replaced by an “Ollama is not running” card. On macOS this card offers Start Ollama and Install Ollama buttons; on Windows and Linux it offers Check again and Install Ollama instead.",
        why: "QuantaMind doesn’t ship its own model runtime — it drives Ollama, a local server, so your models and weights stay on your machine and nothing is sent to the cloud. Auto-launching that server is currently only implemented on macOS.",
        how: "On macOS, “Start Ollama” launches the server in the background (it keeps running after you quit QuantaMind). On Windows/Linux, start Ollama yourself (the Ollama app, or `ollama serve` in a terminal) and click Check again. “Install Ollama” opens ollama.com/download on any OS. Once it’s up, the Models tab lets you pull a model — try llama3.2:1b (~700 MB) for a quick first run.",
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
        id: "context-ceiling",
        heading: "Context ceiling by KV-cache precision + fit verdict",
        what: "Three bars (f16 / q8_0 / q4_0) of the largest context this machine could hold for the model at each cache precision, plus — on Apple Silicon — a “GPU-addressable” line and a coloured weights-fit chip.",
        why: "Capacity is not capability. A big ceiling number is meaningless if the weights don’t even fit on the GPU, and on Apple Silicon macOS only lets the GPU wire down ~66–75% of unified memory — so budgeting against the whole RAM pool over-promises. The fit chip answers the question the bars can’t.",
        how: "Ceilings budget against the GPU’s MEASURED Metal working-set limit (recommendedMaxWorkingSetSize) on Apple Silicon — shown as “~11.8 GB of 16.0 GB usable by the GPU” — not the total pool; off macOS they fall back to a 70%-of-RAM heuristic. The fit chip reads the backend’s FitVerdict: Fits (real headroom) · Tight (weights ≥ 85% of the limit, little room for context) · spills to CPU/swap (weights alone exceed the limit → very slow). When the limit is unmeasured the chip is hidden — never guessed. The bars measure MEMORY ONLY: not prefill speed at long context, and not whether the backend launches there by default. q8 ≈ 2× f16 at negligible quality cost; q4 ≈ 4× at a real quality cost and often slower — planning info, never auto-selected.",
        source: "frontend/src/features/inspector/components/kv/KvCeilingBars.tsx · backend/src/commands/llama/llama_runtime.rs (ctx_ceilings, FitVerdict)",
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
        how: "Counts runs that hit a schema-invalid call, and of those, how many later produced a valid one. If no run ever hit a schema error the metric didn’t apply and shows “—” (never 0) — hover the dash and it says so (“no schema errors — nothing to recover from”), so a clean run’s dash isn’t mistaken for a missing value.",
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
    id: "mcp",
    title: "MCP (real tools)",
    blurb: "Score a model against real MCP tools — a real filesystem or SQLite DB — grading world-state, not the transcript.",
    blocks: [
      {
        id: "mcp-source",
        heading: "MCP as a Tests data source",
        what: "A third data source on the Tests page, alongside Built-in and Custom JSON: select ◉ MCP and the center becomes the connect-and-build flow while the sidebar becomes the MCP task list — reusing the same Run Batch button, scoreboard, Simulator, and Model Results.",
        why: "Simulated-tool tasks score the shape of a call; MCP scores the effect of it against a real server. It's the same engine and scoreboard so the two kinds of test read the same way.",
        how: "The MCP radio flips the eval page into MCP mode (mcpStore.active). Model, iterations (k), and decoy come from the global header/Run Params, not an MCP-specific picker (decoy is forced off for MCP).",
        source: "frontend/src/features/eval/components/manager/EvalManager.tsx · frontend/src/features/mcp/",
      },
      {
        id: "mcp-connect",
        heading: "Connect a server (stdio · probe)",
        what: "Add an MCP server with an id, a command (default npx), and args; quick-add chips scaffold the filesystem and sqlite servers. Connect runs a probe that reports “✓ N tools discovered” or a loud red error.",
        why: "The transport is local stdio JSON-RPC — a child process on your machine, nothing cloud-bound. Probing at setup turns a bad command or a stdout-polluting server into a preflight failure, not a mid-run one.",
        how: "The sqlite chip auto-appends an app-managed scratch DB path; filesystem takes a directory you append. Both need Node/npx. Tool definitions are hash-pinned on first connect and re-diffed on reconnect, so a server that swaps its tools (a rug-pull) trips the pin.",
        source: "backend/src/mcp/transport.rs · backend/src/mcp/registry.rs · frontend/src/features/mcp/components/McpConnectPanel.tsx",
      },
      {
        id: "mcp-world",
        heading: "Test World — seed → act → grade end-state",
        what: "A guided builder whose three sections are the task: an instruction, a disposable world (temp folder with seeded files, or temp SQLite DB with setup SQL), and an answer key (files present/absent/contain, or SELECT … :: expected).",
        why: "Grading the world's actual end-state, never the model's words, is the only reward-hacking-proof signal — a transcript that says “done” is an unverified, gameable claim.",
        how: "Per run: seed a fresh sandbox → spawn the real server scoped to it → let the model drive the tools → grade the oracle against the end-state → tear the sandbox down. A brand-new dir/DB each run is what makes repeats independent. The verdict is strict Pass^k: ready only if EVERY run passed.",
        source: "backend/src/inference/eval/mcp/world.rs · oracle_fs.rs · oracle_db.rs · score.rs",
      },
      {
        id: "mcp-byo",
        heading: "Bring-Your-Own (diagnostic, no answer key)",
        what: "The second authoring door: an instruction + one connected server, with no oracle. It reports “schema-valid X/Y” — whether the model emitted well-formed calls to your real tools — never a pass/fail verdict.",
        why: "Sometimes you just want to smoke-test a live server, not score it. Keeping BYO out of the pass-rate aggregate (blue diagnostic, never green/amber/red) means real and simulated scores are never blended.",
        how: "Runs via run_mcp_byo_batch and lights up the same Simulator/Evaluator/Model Results with DiagnosticStats. Unlike disposable worlds, it targets your REAL server, so it's fail-closed: tool calls are deny-by-default (graded schema-valid but NOT executed) until you tick “Allow tool execution” for the run — that opt-in is the explicit approval the gate requires. Even then, confinement is whatever you scoped the server to (e.g. the directory you passed).",
        source: "backend/src/commands/mcp/run_cmd.rs · frontend/src/features/mcp/components/McpByoBuilder.tsx",
      },
      {
        id: "mcp-safety",
        heading: "Sandboxing, deny-by-default, and secrets",
        what: "Disposable worlds auto-approve their tool calls (you can't gate k automated runs on human clicks); your own real tools default to deny. Seed paths are fs_guard-confined, secrets stay in the OS keychain, and untrusted server output is rendered inert.",
        why: "A test harness that drives real tools has to be safe by construction, not by trust — the seed-path and secret rules are the same invariants the rest of the app follows.",
        how: "fs_guard canonicalizes roots and rejects `..`/symlink escapes; seed paths must be relative (absolute/traversal rejected and redacted). Only env-var NAMES are persisted to mcp_servers.yaml; values live in the keychain. Server stderr/args pass through redact_path before any logging; servers are Tauri-managed and reaped on exit.",
        source: "backend/src/fs_guard/mod.rs · backend/src/inference/mcp/gate.rs · backend/src/persistence/mcp/servers.rs",
      },
      {
        id: "mcp-validate",
        heading: "Validation gate (⇄ qm validate)",
        what: "Every task is validated before it can score — on paste in the builder, and again automatically on qm run/test. The answer key must be solvable (a perfect agent passes) AND discriminating (a do-nothing agent fails); vacuous or contradictory oracles are rejected with named findings.",
        why: "A 2026 τ-bench audit clocked a literal do-nothing agent at 38% pass^k on unvalidated tasks. A broken answer key makes every Pass^k a lie, so there's no bypass flag.",
        how: "`qm validate --collection ./worlds.json` runs the same gate as a report: structural schema, static contradiction/vacuity checks, and (with --live) a real world spawn where a do-nothing agent must fail the real oracle. Missing npx/sqlite3 is Inconclusive with the exact install command, never a fake failure.",
        source: "backend/src/inference/eval/mcp/validate.rs · backend/src/cli/validate.rs",
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
        what: "Export the run-history metrics as CSV (a per-model scoreboard — not an event audit log; the app keeps none).",
        why: "For sharing a regression record or pulling the numbers into a spreadsheet/report.",
        how: "Writes the per-run scores behind the timeline out as a flat CSV.",
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
        what: "Per model: exact weights + KV cache at the run’s context length, vs the cap, with a pressure flag and the KV-cache precision it was graded at.",
        why: "Partial offload is the silent killer of local-agent latency; the readiness verdict needs a truthful fit test, not a guess.",
        how: "Weights are the exact on-disk bytes (never estimated); the KV cache uses the canonical formula from the model’s real dims at the run’s context length. Fits = total ≤ cap; pressure = fits but ≥ 85% of the cap (a soft Conditional note). A llama.cpp model is graded at the KV precision its launch would actually use here — under memory pressure a q8_0 cache (≈half the cache memory), and the verdict then carries an explicit advisory (“fits with Q8 KV cache”). Ollama/MLX stay f16 (their cache type isn’t verifiable from here). Any missing input ⇒ “not measured” (never a guessed fit).",
        formula: "total = weights_bytes + kv_cache_bytes(precision)\nfits = total ≤ cap · pressure = total ≥ 0.85 × cap",
        source: "backend/src/inference/eval/readiness/vram_fit.rs (estimate)",
      },
      {
        id: "right-sizing",
        heading: "Right-Sizing",
        what: "For each model family you assessed with ≥2 quants, the smallest variant still usable on your hardware vs the largest — with measured percent reductions.",
        why: "Right-sizing the quant is the biggest lever on local-inference memory and cost, but only if the smaller variant is still Ready. This does that comparison for you so you can drop down without shipping something broken.",
        how: "Groups the ranked verdicts by family + parameter size; per group, baseline = largest weights, pick = smallest that’s still Ready (or Conditional, flagged). Size % is the exact on-disk saving; memory % shows only when both fits were measured at the SAME KV precision (a q8-vs-f16 comparison is omitted, never faked); Pass^k delta is in percentage points. Percent only — no dollar figures. Host-specific, so it’s never published.",
        source: "backend/src/inference/eval/readiness/rightsizing/right_size.rs",
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

  {
    id: "cli",
    title: "CLI (qm)",
    blurb: "The headless face of QuantaMind — connect, run, and gate CI on the same engine as this app.",
    blocks: [
      {
        id: "cli-connect",
        heading: "Connect & use — 3 commands to a verdict",
        what: "The quickstart: build the binary, let doctor find a runnable backend, and get a real Ready/Conditional/NotReady verdict — no flags to memorize (interactive pickers cover model/collection/thinking in a terminal).",
        why: "The fastest path from a fresh checkout to a verdict; every later command builds on the same three ideas — a backend, a model, a collection.",
        how: "Syntax notation (man-page style): UPPERCASE words are placeholders you replace; [square brackets] mark optional arguments; a|b are alternatives. Copy-paste examples below are bracket-free and runnable as-is. `qm COMMAND --help` prints any command's full flag reference.",
        formula: "# 1) build once (a second binary on this same crate)\ncargo build --bin qm\n\n# 2) find a runnable backend (Ollama/llama.cpp/MLX/vLLM/SGLang)\ntarget/debug/qm doctor\n\n# 3) zero-config first verdict (writes ./qm.json; later runs need no flags)\ntarget/debug/qm init\n\n# environment variables (never pass secrets as arguments):\n#   QM_BASE     endpoint URL, e.g. https://vllm.internal:8000\n#   QM_MODEL    model name\n#   QM_API_KEY  bearer key for vLLM/SGLang (env or OS keychain only)",
        source: "backend/src/bin/qm/main.rs · docs/cli/README.md",
      },
      {
        id: "cli-doctor",
        heading: "Setup ⇄ qm doctor — is anything runnable?",
        what: "The first-run health check (the same probe behind the 'Ollama is not running' card): probes all five backends (Ollama :11434, llama.cpp :8081/:8080, MLX :8082, vLLM :8000, SGLang :30000) for reachability, served models, credentials, native tool-calling, and version.",
        why: "The first-run wall is step 1 — a backend that's up but has zero models pulled looks green everywhere else. Doctor's bar is RUNNABLE (reachable + has a model + credential OK), not merely reachable, so `qm doctor && qm run` gates correctly in scripts.",
        how: "Every failure prints the exact fix command, never runs it: a down server → '[QM-BACKEND-UNREACHABLE] … start it: ollama serve'; a reachable-but-empty server → '[QM-NO-MODELS] … ollama pull MODEL'; a rejected key → '[QM-UNAUTHORIZED] check QM_API_KEY'; a key over plain http is WITHHELD ('[QM-INSECURE-KEY]'). `--json` puts the machine-readable report alone on stdout (fix lines go to stderr, so piping to jq never breaks).",
        formula: "SYNOPSIS\n  qm doctor [--backend BACKEND] [--base URL] [--model MODEL] [--json]\n\nPLACEHOLDERS\n  BACKEND  one of: ollama | llama_cpp | mlx | vllm | sglang (omit = scan all five)\n  URL      endpoint override for the targeted backend (env QM_BASE)\n  MODEL    also probe native tool-calling for this model (env QM_MODEL)\n\nEXIT STATUS\n  0  at least one backend is RUNNABLE     3  nothing runnable\n  2  bad arguments\n\nEXAMPLES\n  qm doctor\n  qm doctor --backend ollama --model qwen2.5:3b\n  qm doctor --json | jq '.backends[0]'",
        source: "backend/src/cli/doctor/",
      },
      {
        id: "cli-init",
        heading: "Setup ⇄ qm init — zero-config first run",
        what: "Auto-detects the first runnable backend, writes a non-secret `qm.json` (backend, model, collection, profile), then runs the suite — install → real verdict in one command.",
        why: "The fastest path from nothing to a verdict; afterwards a bare `qm run` needs zero flags because it reads `qm.json`.",
        how: "Detection reuses the doctor scan. If nothing is runnable it exits 3 with '[QM-NO-RUNNABLE] … run `qm doctor`'. The file never stores an API key (keys stay in env/keychain).",
        formula: "SYNOPSIS\n  qm init [--json]\n\nFILES\n  ./qm.json   { \"backend\", \"model\", \"collection\", \"profile\" } — no secrets\n\nEXIT STATUS\n  follows the verdict: 0 Ready · 10 Conditional · 20 NotReady · 3 nothing runnable\n\nEXAMPLE\n  qm init && echo \"model is agent-ready\"",
        source: "backend/src/cli/init/",
      },
      {
        id: "cli-tests-map",
        heading: "Tests page sidebar ⇄ CLI flags — every control, one flag",
        what: "The full map from the Tests page's left sidebar to the CLI. Every control maps to exactly one flag, and the COLLECTION KIND picks the command: a built-in id runs `qm run`, a custom collection file or an MCP world file runs `qm test`.",
        why: "The sidebar and the CLI drive the same engine with the same knobs — once you know this mapping you can reproduce any UI batch in a terminal or CI verbatim, with nothing renamed and nothing missing.",
        how: "Three collection kinds, three examples below. Built-in: pass the id (easy-coding, medium-coding, …). Custom: export your collection to a .json file and pass its path. MCP world: pass the world .json the MCP builder authors — it is validated automatically before any model runs (see qm validate). Everything else is one flag per sidebar control.",
        formula:
          "UI control (sidebar)          CLI flag\n" +
          "───────────────────────────   ──────────────────────────────────────────\n" +
          "Collection — built-in         qm run  --collection easy-coding\n" +
          "Collection — custom file      qm test --collection ./my_suite.json\n" +
          "Collection — MCP world file   qm test --collection ./worlds.json\n" +
          "Target model                  --model MODEL   (one model per CLI run)\n" +
          "Iterations (k)                --k N\n" +
          "Max Steps                     --max-steps N\n" +
          "Enable Decoy Tools + count    --decoy N\n" +
          "Native FC / Prompt-based      --mode native | prompt_based | both\n" +
          "Thinking Budget               --thinking lean | standard | deep\n" +
          "Tier                          --tier easy | medium | hard | extreme\n" +
          "Global params (gear ⚙)        --temperature · --top-p · --top-k · --num-predict\n" +
          "                              --repeat-penalty · --seed · --num-ctx\n" +
          "\n" +
          "EXAMPLES (copy-paste, then swap MODEL)\n" +
          "  qm run  --backend ollama --model MODEL --collection easy-coding --k 5 --max-steps 8\n" +
          "  qm test --backend ollama --model MODEL --collection ./my_suite.json --mode both --k 3\n" +
          "  qm test --backend ollama --model MODEL --collection ./worlds.json   # MCP world",
        source: "frontend/src/shared/cli/qmCommand.ts (buildRunCommand) · backend/src/cli/run/",
      },
      {
        id: "cli-run",
        heading: "Tests page ⇄ qm run — the readiness verdict",
        what: "Runs a BUILT-IN collection (the same 27 this app ships: easy/medium/hard/extreme tiers + boundary/noisy sets) through the same agentic engine, and prints the Ready/Conditional/NotReady verdict with its exact ✗ blocking / ! condition reasons.",
        why: "This is the Tests page's Run Batch as an exit code: `--fail-on` decides which verdicts fail a pipeline, so a team can gate a deploy on model readiness.",
        how: "`--mode both` runs the native function-calling pass AND the prompt-based pass (one verdict row each). `--thinking standard|deep` is guarded per model+server — a model/server that can't actually reason exits 2 with '[QM-THINKING-UNSUPPORTED]' and the per-engine fix, instead of silently behaving like lean. A run that ERRORS (backend fault mid-run) exits 11 (inconclusive — retry), never a fake NotReady. In a terminal, omitted --collection/--model/--backend/--thinking open numbered pickers (collections listed with tier + domain); over SSH/CI there is never a prompt.",
        formula: "SYNOPSIS\n  qm run [--backend BACKEND] [--model MODEL] [--collection COLLECTION]\n         [--mode prompt_based|native|both] [--tier easy|medium|hard|extreme]\n         [--thinking lean|standard|deep] [--k N] [--max-steps N] [--decoy N]\n         [--profile PROFILE] [PARAMS] [--fail-on conditional|notready|never]\n         [--junit PATH] [--save-report PATH] [--json]\n\nPLACEHOLDERS\n  BACKEND     ollama | llama_cpp | mlx | vllm | sglang (default: qm.json, then ollama)\n  MODEL       served model name (env QM_MODEL; default: qm.json, else terminal picker)\n  COLLECTION  built-in id (e.g. easy-coding, medium-coding) or a .json file path\n  N           strict pass^k count (--k) · step cap (--max-steps) · decoy count (--decoy)\n  PROFILE     general-agent | rag-assistant | coding-agent, or a ReadinessProfile .json\n  PARAMS      --temperature/--top-p/--top-k/--num-predict/--repeat-penalty/--seed/--num-ctx\n              (eval is greedy temp-0 by default; pass these to sample, like the GUI)\n\nEXIT STATUS\n  0 Ready · 10 Conditional · 11 Inconclusive (retry) · 20 NotReady\n  2 bad args or capability mismatch · 3 unreachable / model not served\n\nEXAMPLES\n  qm run --backend ollama --model qwen2.5:3b\n  qm run --collection medium-coding --thinking standard --k 1 --max-steps 8\n  qm run --model qwen2.5:3b --temperature 0.7 --decoy 2 --save-report run.json",
        source: "backend/src/cli/run/",
      },
      {
        id: "cli-test",
        heading: "Tests page ⇄ qm test — your own collection",
        what: "Same engine as `run`, but for a collection FILE you authored (the schema the Tests page edits: a v2 collection object or a raw ToolTask array, JSON, 1 MB cap) — defaulting to `--mode both` and printing a native-vs-prompt scoreboard (pass^k · tasks · steps · effort · top-error per mode).",
        why: "The scoreboard makes the native-vs-prompt split visible — e.g. a model whose native pass reports-in-prose (0/2) while its prompt pass aces (2/2) — the exact difference a bare verdict hides.",
        how: "The collection path is shown by basename only (never your full filesystem path). A bad/missing/malformed file exits 2 with '[QM-BAD-COLLECTION]' naming the parse error. All `run` flags apply.",
        formula: "SYNOPSIS\n  qm test --collection FILE [--backend BACKEND] [--model MODEL]\n          [--mode prompt_based|native|both] [--tier TIER] [--thinking TIER]\n          [--k N] [--fail-on POLICY] [--junit PATH] [--save-report PATH] [--json]\n\nPLACEHOLDERS\n  FILE  a .json collection: a v2 object { name, domain, tier, tasks: [...] }\n        or a raw ToolTask array — the same schema the Tests page authors\n\nEXAMPLES\n  qm test --collection ./my_suite.json --backend ollama --model qwen2.5:3b\n  qm test --collection ./my_suite.json --mode both --k 1 --json",
        source: "backend/src/cli/run/ (render_scoreboard)",
      },
      {
        id: "cli-cliff",
        heading: "Audit page ⇄ qm cliff — the Context Stress Test",
        what: "The Audit tab's context-cliff probe, headless: ramps prompt depth toward `--max-tokens` across `--steps` rungs, sweeps the needle across positions, and classifies where tool-calling collapses.",
        why: "A model's usable context for tool use is part of its readiness; the CLI makes the probe scriptable and repeatable (greedy temp-0 decoding, so the same model + collection reproduces the same verdict).",
        how: "Prints one line per rung — '~N tok · accuracy X% (passed/trials)', the sample tally shown only when measured — then a STATUS line. A rung that would exceed the context window is dropped, never scored; a sample too small to tell a cliff from noise is INCONCLUSIVE (exit 11), never a guessed verdict; a baseline already failing is 'broken' (a tool-call failure, not a context limit).",
        formula: "SYNOPSIS\n  qm cliff [--backend BACKEND] [--model MODEL] [--collection COLLECTION]\n           [--max-tokens N] [--steps K] [--mode prompt_based|native]\n           [--thinking lean|standard|deep]\n           [--source corporate_policy|system_logs|financial_ledger] [PARAMS] [--json]\n\nPLACEHOLDERS\n  N       deepest rung's target prompt tokens (default 4096)\n  K       ladder rungs including the unpadded baseline (default 4, min 2)\n  --mode  probe the native tool-calling path (default prompt_based)\n  --thinking  reasoning scratchpad preset (default lean = off); scales with each\n          rung's depth through the run tiers' budget table\n  PARAMS  the 7 sampling flags (see run). Greedy temp-0 by default so the probe\n          reproduces; pass --temperature (etc.) to sample.\n\nEXIT STATUS\n  0 no cliff · 10 collapsed (20pp drop AND its Wilson/Newcombe interval\n    excludes zero; one-task-driven collapses are labeled low confidence)\n  11 inconclusive (sample can't resolve the margin) · 20 broken baseline\n\nEXAMPLES\n  qm cliff --backend ollama --model qwen2.5-coder-7b-instruct:q4_k_m\n  qm cliff --model qwen3.5:9b --max-tokens 3000 --steps 3 --mode native --json",
        source: "backend/src/inference/eval/cliff/engine.rs · backend/src/cli/cliff.rs",
      },
      {
        id: "cli-report",
        heading: "Agent Report page ⇄ qm report — one run, many bars, offline",
        what: "Re-assesses a SAVED run (written by `run`/`test --save-report`) against a readiness profile — the Agent Report's verdict card, from the terminal, with no backend and no re-inference.",
        why: "Hold one measurement up to many bars in milliseconds: the same run can be Ready under a lenient dev profile and NotReady under a strict launch gate — the profile is the policy, the run is the fact.",
        how: "`--profile` takes a built-in id or your own ReadinessProfile JSON — the file carries EVERY threshold the Agent Report page shows: min_pass_k (Pass^k), forbid_infinite_loop (Infinite Loops), forbid_hallucinated_completion (Fake-Done), require_native_fc (Native FC), require_full_vram (Full VRAM), max_avg_steps (Max Steps), min_context_tokens (Min Context), max_ms_per_step (Max Latency), required_tier. The page's Equivalent-CLI preview writes that JSON for you with the exact thresholds currently active, so the CLI grades the same bar. Prints the same verdict card as `run`: status, pass^k, run tally, and the exact ✗/! reasons. Exits on the verdict, subject to --fail-on.",
        formula: "SYNOPSIS\n  qm report --report FILE [--profile PROFILE] [--fail-on POLICY]\n            [--junit PATH] [--json]\n\nPLACEHOLDERS\n  FILE     a saved raw report from `qm run … --save-report FILE`\n  PROFILE  built-in id or a ReadinessProfile .json (your own bar)\n\nEXAMPLES\n  qm run --model qwen2.5:3b --save-report run.json   # measure once\n  qm report --report run.json --profile general-agent\n  qm report --report run.json --profile ./strict.json --fail-on notready",
        source: "backend/src/cli/run/ (assess_saved)",
      },
      {
        id: "cli-prompt",
        heading: "Workspace page ⇄ qm prompt — free-form generation",
        what: "The one non-eval command: run a system + user prompt against a model with inference params and STREAM the output — the headless twin of the Workspace Run button (same inference path, so the CLI and GUI agree).",
        why: "Everything else in the CLI evaluates fixed collections; `qm prompt` is for the interactive case — quick generations, scripting a model, piping output — with the exact same global params the Workspace uses.",
        how: "The user prompt comes from --user or, omitted, from STDIN (pipe it, or type + Ctrl-D). Tokens stream to stdout; [QM-*] diagnostics + a `[QM-DONE] N tokens` line go to stderr, so `qm prompt … | tee out.txt` captures only the generation. The 7 param flags are shared with run/test/cliff (max_tokens → --num-predict). Exit 0 ok · 3 unreachable/model-not-served · 2 bad args.",
        formula: "SYNOPSIS\n  qm prompt [--backend BACKEND] [--model MODEL] [--system '…'] [--user '…']\n            [PARAMS]\n\nPARAMS (shared with run/test/cliff — mirror the GUI global params)\n  --temperature 0.0–2.0   --top-p 0.0–1.0   --top-k N\n  --num-predict N (max tokens)   --repeat-penalty 0.0–2.0\n  --seed N   --num-ctx N\n\nEXAMPLES\n  echo \"Name three primary colors.\" | qm prompt --model qwen2.5:7b\n  qm prompt --model qwen2.5:7b --system \"You are terse.\" --user \"Summarize X\" --temperature 0.7",
        source: "backend/src/cli/prompt.rs (reuses run_prompt_inner)",
      },
      {
        id: "cli-validate",
        heading: "Tests/MCP pages ⇄ qm validate — prove the test before trusting it",
        what: "Validates a collection or WORLD file the way this app's import validation does — structural schema, reachability (a perfect scripted agent succeeds), discrimination (a do-nothing agent fails), plus MCP-world checks: static (vacuous or contradictory oracle, escaping seed paths) and live (spawn the real server, grade the oracle on the untouched seed — it must fail).",
        why: "An eval is only as honest as its answer key: a 2026 τ-bench audit measured a literal do-nothing agent at 38% pass^k on unvalidated tasks. The same pipeline gates `qm run`/`test` on every uploaded file — an invalid collection can never start testing (no bypass flag), so a green pass^k always means something.",
        how: "World files are plain JSON (the same shape this app's MCP builder authors): an array of { instruction, world (fs files / db setupSql), oracle (assert_present/assert_content or assert_contains) }. Each run spawns a REAL MCP server (@modelcontextprotocol/server-filesystem or mcp-server-sqlite-npx via npx) in a throwaway sandbox and grades the end-state of the files/rows, never the model's words. Needs npx (Node) for any world; sqlite3 for db worlds — missing deps print the install command before any model runs. A server that dies mid-run is Inconclusive (retry), never a fake model failure.",
        formula: "SYNOPSIS\n  qm validate [--collection ID_OR_FILE] [--live-world true|false] [--json]\n\nPLACEHOLDERS\n  ID_OR_FILE   a built-in id, a collection .json, or a WORLD .json\n  --live-world spawn each world + run the do-nothing check (default true; needs npx)\n\nEXIT STATUS\n  0 valid · 10 warnings only · 20 invalid (fix the ✗ findings)\n  11 worlds not live-checkable (npx/sqlite3 missing) · 2 bad file\n\nWORLD FILE (author a real-tool test)\n  [{ \"name\": \"summarize\",\n     \"instruction\": \"Read notes.txt, write summary.md containing 'alpha'.\",\n     \"world\":  { \"type\": \"fs\", \"files\": [{ \"path\": \"notes.txt\", \"content\": \"alpha shipped\" }] },\n     \"oracle\": { \"assert_present\": [\"summary.md\"], \"assert_content\": [[\"summary.md\", \"alpha\"]] } }]\n\nEXAMPLES\n  qm validate --collection ./worlds.json      # prove it before running\n  qm run --collection ./worlds.json --model qwen2.5:3b   # gate runs automatically",
        source: "backend/src/cli/validate.rs · backend/src/inference/eval/mcp/validate.rs",
      },
      {
        id: "cli-cicd",
        heading: "CI/CD — gate a pipeline on model readiness",
        what: "Two ways to wire the verdict into CI: plain shell (the exit-code contract works in any runner) or the bundled GitHub Action `.github/actions/qm-eval`, which builds qm, runs the suite, writes a JUnit report for the test panel, and uploads the JSON report as an artifact.",
        why: "A model regression should fail a build the same way a unit test does. `--fail-on` is the team policy knob: strict gates block on Conditional; soft gates surface findings without blocking; exit 11 means retry (infra), never 'the model is bad'.",
        how: "Secrets discipline: the endpoint is an input, but the API key is passed ONLY as the QM_API_KEY environment variable from a secret (action inputs are logged). GitHub-hosted runners can't host a local model, so CI targets a remote vLLM/SGLang endpoint; self-hosted runners can use Ollama/llama.cpp. Full recipes incl. vault/OIDC injection: docs/ci/README.md.",
        formula: "# any CI (plain shell) — doctor gates, run verdicts:\nqm doctor --backend vllm --base \"$QM_BASE\" || exit 3\nqm run --backend vllm --model \"$QM_MODEL\" --fail-on notready --junit qm-junit.xml\n\n# GitHub Actions (the bundled composite action):\n- uses: actions/checkout@v4\n- uses: ./.github/actions/qm-eval\n  with:\n    backend: vllm\n    base-url: ${{ secrets.VLLM_URL }}\n    model: qwen3-32b\n    collection: easy-coding\n    fail-on: notready        # team policy\n    ci-profile: fast         # k=1 PR gate; \"full\" for the nightly\n  env:\n    QM_API_KEY: ${{ secrets.QM_API_KEY }}   # env from a secret — never an input",
        source: ".github/actions/qm-eval/action.yml · docs/ci/README.md",
      },
    ],
  },
];

/// Render one reference block into the markdown `DocMarkdown` understands. The heading is level-2
/// so it lands in the right-rail TOC; its slug is the `#docs-<page>--<slug>` deep-link anchor.
function blockToMarkdown(b: ReferenceBlock): string {
  const parts = [
    `## ${b.heading}`,
    `**What it is** — ${b.what}`,
    `**Why it matters** — ${b.why}`,
    `**How it works** — ${b.how}`,
  ];
  if (b.formula) parts.push("```\n" + b.formula + "\n```");
  if (b.source) parts.push(`Source: \`${b.source}\``);
  return parts.join("\n\n");
}

/// One reference section → one `DocPage` (id `reference-<sectionId>`), its blocks stacked as
/// markdown. Reuses the Docs renderer + search verbatim — the per-feature reference becomes part
/// of the same ⌘K index as the guides.
function sectionToPage(s: ReferenceSection): DocPage {
  const body = [`# ${s.title}`, `_${s.blurb}_`, ...s.blocks.map(blockToMarkdown)].join("\n\n");
  return { id: `reference-${s.id}`, title: s.title, description: s.blurb, body };
}

/// Fold the whole per-feature reference into a single "Reference" sidebar section for the Docs tab.
export function referenceToDocSection(sections: ReferenceSection[] = REFERENCE_SECTIONS): DocSection {
  return { id: "reference", title: "Reference", pages: sections.map(sectionToPage) };
}

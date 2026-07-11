/// In-app help copy for the Eval/Audit tools and metrics — a single source of
/// truth so the same explanation appears wherever a metric is shown (the Model
/// Results tooltips and the Audit history popup).

export interface Help {
  title: string;
  body: string;
}

/// What each tool does / when to use it / how it scores. Shown in an InfoButton.
export const TOOL_HELP = {
  evalManager: {
    title: "Tests Manager",
    body: "Pick a collection (a built-in suite or your own), choose which models to test, set Iterations (k) and Max Steps, then run the batch. This is the control panel — it doesn't score anything itself; it dispatches the run that fills the Simulator and Model Results.",
  },
  simulator: {
    title: "The Simulator (Batch Scoreboard)",
    body: "A live grid of every model × task as the batch runs: each cell turns pass/fail the moment that task finishes. Use it to watch progress and spot which tasks a model struggles with. Click a cell to open that task in the Evaluator below.",
  },
  evaluator: {
    title: "The Evaluator ",
    body: "Drill into ONE task end-to-end: the exact system prompt sent, the model's raw output, the tool call parsed from it, and the verdict (parsed? right tool? right args?). Use it when a cell failed and you need to see why — the eval is never a black box.",
  },
  performanceMatrix: {
    title: "Model Results",
    body: "Per-model summary of the last run — Pass^k, Avg Steps, Effort and the Top error for each model. Click a row to focus that model in the Simulator/Evaluator. Hover a column header for what it measures.",
  },
  boundaryPanel: {
    title: "Safety & Boundaries (Category K)",
    body: "Only the three built-in Category-K collections — boundary-healthcare, boundary-banking, boundary-coding — carry safety tasks, so this panel appears only after you run one of them (other collections have no safety axis). It measures prompt-injection resistance (Attack arm) alongside an over-refusal control (BenignControl arm, an injection-shaped but legitimate call the model should still make). The gate passes only when resistance is high AND over-refusal is low — a refuse-everything model fails. Over-refusal counts genuine refusals only; capability failures (loops/truncations) are excluded. When a config takes a trapped action it's attributed to the model (followed the injection) vs the config (its context window evicted the safety guard). Config-attribution is valid ONLY for that truncation case — injection resistance is largely not a serving-config property, so a non-truncation failure needs an out-of-band guardrail layer (CaMeL/FIDES/Progent-style) this tool doesn't provide, not config tuning. Kept off the capability Pass^k axis. Resistance is a FLOOR on vulnerability, not a ceiling: it's a fixed, known-injection set (adaptive attacks do materially better), and much of any resistance is 'security by incompetence' — the model failing through inability, not defense.",
  },
  auditHistory: {
    title: "Results History",
    body: "A regression timeline of past batch runs for this collection — each model's pass-rate / composite over consecutive runs, so you can catch when a model (or a prompt change) regresses. Hover a point for its run number and score. It tracks the same metrics the Model Results table reports: Pass^k (consistency), Effort (tokens to succeed), and the dominant Top error.",
  },
  contextCliff: {
    title: "Context Stress Test",
    body: "Runs the chosen dataset at growing prompt lengths and graphs where tool-call accuracy collapses — the point where long context breaks tool use. Use it to find a model's usable context window for tool use. Padding is approximate (≈tokens), so the depth is indicative, not a tokenizer count.",
  },
  iterations: {
    title: "Iterations (k)",
    body: "How many times each Multi-Step (agentic) task is re-run — the k in Pass^k. A real agent loops many steps where small failures compound, so passing once isn't enough: with k=5 a task runs 5× and the model only counts as reliable if it passes consistently (shown as passes/total, e.g. 4/5). Higher k = a stricter reliability bar (and longer runs); k=1 = run once, no consistency check. No effect on single-turn tasks (they always run once).",
  },
  decoys: {
    title: "Anti-Saturation (Decoy Tools)",
    body: "Shuffles N never-correct 'decoy' tools into every agentic task's presented tool list — plausible-looking distractors the model must avoid. It tests whether a model is misled into calling the wrong tool as the menu grows, resisting benchmark saturation. Off (default) leaves each task's authored decoys untouched. No effect on single-turn tasks.",
  },
  thinkingBudget: {
    title: "Thinking Budget",
    body: "How many tokens a reasoning model may spend thinking before it must emit its tool call, per turn. Reasoning eats the same output budget as the answer, so too small a budget starves the answer and the run fails as Truncated even for a capable model. Lean / Standard / Deep are fixed presets (not a free slider) so results stay reproducible and comparable — the same tier is the same number on every machine. Deep is a large fixed cap for the chattiest models, never 'unlimited'. The budget is the SAME on every machine (reasoning length depends on the model and task, not your RAM); only the context window scales with hardware. A model that burns its whole budget without answering is scored 'Reasoning-overrun' (raise the preset), not out-of-memory. Verdicts are labeled with the preset they were run under.",
  },
} satisfies Record<string, Help>;

/// How each metric is computed. Used both in InfoButtons and as clip-safe Tooltip
/// labels on the dense metric column headers.
export const METRIC_HELP = {
  passK: {
    title: "Pass^k",
    body: "Reliability across repeats: passes ÷ total runs when each agentic task is run k times. Higher = more consistent. For single-turn tasks it's the composite accuracy instead.",
  },
  avgSteps: {
    title: "Avg Steps",
    body: "Mean number of agentic turns (tool calls) a model took to finish a task. Lower is leaner; single-turn tasks have no steps (—).",
  },
  effort: {
    title: "Effort",
    body: "Mean output tokens on the runs that SUCCEEDED — the token cost of getting it right. Failed runs are excluded so a model isn't rewarded for giving up early.",
  },
  tokensPerTask: {
    title: "Tokens/Task",
    body: "Total generated tokens ÷ completed tasks — the amortized cost of one completion, INCLUDING the tokens burned on failed runs. Unlike Effort (successes only), this charges the 'waste tax' of unreliability, so it's always ≥ Effort and the gap shows how much a flaky model really costs. Output tokens only; lower is better; '—' when nothing completed. Not comparable across thinking vs terse models.",
  },
  topError: {
    title: "Top error",
    body: "The model's dominant agentic failure mode across the collection: Loop Cap (hit the step limit), Fake Done (claimed success without the end-state), Bad Schema (burned its recovery budget on schema-invalid calls), or Malformed (unparseable tool JSON). 'None' means no failures dominated. Hover the ⓘ next to the badge for the full count of all four modes.",
  },
  schemaResil: {
    title: "Schema resilience",
    body: "Of the runs that hit a semantic schema error (missing/typed-wrong param), the share that recovered — emitted a valid call after the injected correction. '—' means no run ever hit one, so the metric didn't apply.",
  },
  cliffDepth: {
    title: "Context Limit",
    body: "The measured context length (real prompt tokens) at which this model's accuracy collapses, from the Context Stress Test in the Audit tab. 'Accuracy' here is the composite tool-call score (parse + tool-selection + argument + abstention), 0–100%, scored at each rung. The cliff is the first rung that drops ≥20pp below the unpadded baseline. It feeds the Agent-Readiness verdict (a model that breaks down before your app's context needs is downgraded). States: 'Run probe ↗' until measured; 'N tok' = a measured cliff depth; '✓ no cliff' = a HEALTHY baseline (≥50%) that held the whole tested range; 'fails from start' = the baseline itself was below 50%, so the model is broken at the smallest context — a tool-call failure, not a context-length limit (not the same as 'no cliff').",
  },
  passRate: {
    title: "Pass rate",
    body: "Aggregate passes ÷ total runs across the whole collection, as a percentage.",
  },
  cliffAccuracy: {
    title: "Accuracy (per rung)",
    body: "The composite tool-call score at this context depth, 0–100%. Single-turn tasks score the mean of four sub-metrics — parse rate (valid JSON call), tool selection (right tool), argument accuracy (right parameters), and abstention (correctly made no call when none was needed); agentic tasks score on JSON well-formedness alone (did the model emit a parseable tool call). The two groups are blended by task count. Each rung is measured at three needle positions (start/middle/end of the padding) and the row shows the WORST position — robust means correct everywhere. A rung is a Pass at ≥50%; the cliff is the first rung that drops ≥20 percentage points below the unpadded baseline.",
  },
} satisfies Record<string, Help>;

/// "Title — body" for a clip-safe Tooltip label on a metric column header.
export function metricTitle(key: keyof typeof METRIC_HELP): string {
  const h = METRIC_HELP[key];
  return `${h.title} — ${h.body}`;
}

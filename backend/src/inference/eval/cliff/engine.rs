use super::budget::{CliffBudget, CLIFF_ANSWER_TOKENS};
use super::padding::{build_padding, inject_at_depth};
use super::presets::CliffSource;
use super::stats;
use crate::inference::eval::agentic::difficulty::passk::ThinkPreset;
use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::model_turn::{ModelTurn, Progress};
use crate::inference::eval::readiness::types::{CliffConcentration, CliffStatus};
use crate::inference::eval::toolcall::eval::{aggregate, TaskResult};
use crate::inference::eval::toolcall::parse::extract_calls;
use crate::inference::eval::toolcall::prompt::{build_system_for, TerminalGuidance};
use crate::inference::eval::toolcall::score::{score, verdict_passed, Verdict};
use crate::inference::eval::toolcall::tasks::{is_agentic, ToolTask};
use crate::inference::generate::generate_options::{GenerateOptions, EVAL_REPEAT_PENALTY};
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Byte seed per target token. The model's REAL `prompt_eval_count` is measured
/// afterward and the padding rebuilt proportionally — this 4:1 ratio is only the
/// starting estimate, never the reported depth.
const BYTES_PER_TOKEN: usize = 4;
/// Max proportional rebuilds after the first sweep (verify-and-adjust). Kept at 1:
/// once the byte→token rate is learned (see `run_cliff_with`), each rung sizes
/// correctly on the first sweep, so a rebuild is a rare safety net, not the norm.
const MAX_ADJUST_ATTEMPTS: usize = 1;
/// Accept a rung when the measured depth is within ±5% of the requested target.
const ADJUST_TOLERANCE: f64 = 0.05;
/// Output token floor per probe turn — only a tool call is expected, never prose.
/// A thinking run adds a depth-banded scratchpad on top (see `budget::CliffBudget`);
/// the per-rung cap is threaded through as `max_output`, never read from this const
/// past `probe_rung`.
const MAX_OUTPUT: u32 = CLIFF_ANSWER_TOKENS;
/// The baseline rung must clear this composite or the run is `Broken` (the model
/// can't even do the task unpadded — a cliff number would be meaningless).
const BASELINE_PASS: f64 = 0.5;
/// A deeper rung is a cliff when its composite falls this far below the baseline.
const COLLAPSE_MARGIN: f64 = 0.2;
/// "No context ceiling to check against" — for scripted/test turns, whose fake token counts
/// have no real window behind them. A real run always passes the backend's effective window.
pub const NO_CTX_LIMIT: u32 = u32::MAX;
/// The needle is injected at these fractional depths — front / middle / back, never
/// tail-only (the tail tests recency, the model's strongest position). Three
/// positions keep the probe affordable; mid-document is where models actually fail.
pub const DEFAULT_DEPTHS: [f32; 3] = [0.1, 0.5, 0.9];
/// Caps on the per-rung trace: how many tasks to retain, and the char length of each
/// captured system prompt / model output. The probe keeps a trace for EVERY task at
/// EVERY rung (pass and fail) so the UI's per-step "View trace" can show the exact
/// system prompt + output — bounded here so a large collection can't haul unbounded
/// text through IPC.
const MAX_TRACE_TASKS: usize = 30;
const MAX_OUTPUT_CHARS: usize = 2000;
/// The padded user prompt is huge by design; keep its head + tail (eliding the middle)
/// so the user can see the padding that was injected — and the needle, whether it sits
/// at the front or the back — without hauling the full multi-KB context through IPC.
const MAX_PROMPT_CHARS: usize = 6000;

/// One model output at one needle position within a rung.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TraceOutput {
    /// Fractional needle position (front/middle/back); `0.0` for the unpadded baseline.
    pub depth: f32,
    /// The exact user prompt sent at this position: the synthetic padding with the task
    /// instruction (the "needle") injected at `depth`. Head+tail-capped — this is what
    /// the model actually read, so "View trace" shows the padding, not just the output.
    pub prompt: String,
    /// The raw model completion (char-capped), verbatim — prose, refusal, JSON, anything.
    pub output: String,
    /// Did this output PASS the cliff yardstick (single-turn: fully correct; agentic:
    /// emitted a well-formed tool call)?
    pub passed: bool,
    /// Total tokens this cell decoded (backend `eval_count`). `None` = the backend
    /// reported no count — never fabricated.
    #[serde(default)]
    pub decoded: Option<u32>,
    /// Reasoning-channel tokens, measured only where the backend tokenizes them
    /// (llama.cpp `/tokenize` over `reasoning_content`); `None` elsewhere — the split
    /// is display-only, headroom math never depends on it.
    #[serde(default)]
    pub thinking: Option<u32>,
    /// Did generation stop at the output cap (`finish_reason == "length"`)? A cell
    /// that died here is HARNESS-limited, not model-limited. `None` = not reported.
    #[serde(default)]
    pub cap_hit: Option<bool>,
}

/// The full trace for one task at one rung: every needle position's padded input +
/// output. Captured for every task (pass and fail) so a rung's "View trace" shows what
/// the model saw and emitted, not only the failures. The system prompt is the same
/// boilerplate + tool schemas every turn, so it's deliberately NOT carried here.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskTrace {
    pub task_id: String,
    pub outputs: Vec<TraceOutput>,
}

/// One generation's raw trace at a single position, before grouping by task.
struct PosTrace {
    task_id: String,
    prompt: String,
    output: String,
    passed: bool,
    decoded: Option<u32>,
    thinking: Option<u32>,
    cap_hit: Option<bool>,
}

/// One task's pass count at one rung, across every swept needle position. UNCAPPED —
/// unlike `trace` (`MAX_TRACE_TASKS`), this is an id plus two ints per task, so the
/// verdict layer can always see WHICH tasks drove a rung, however large the collection.
/// Fed from the same `cliff_failed` yardstick as the trace's `passed` flag — one source.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskTally {
    pub task_id: String,
    pub passed: u32,
    pub trials: u32,
    /// FAILING cells of this task that died at the output cap (`finish == length`).
    /// When every failure in a rung is a cap-hit, the rung is budget-limited, not a
    /// model collapse — the verdict reads this.
    #[serde(default)]
    pub failed_cap_hits: u32,
    /// The tightest headroom (‰ of the cap left unused) over this task's PASSING
    /// cells — the early-warning signal: a pass at <150‰ headroom is likely to fail
    /// at the next rung. `None` = no passing cell reported a count.
    #[serde(default)]
    pub min_pass_headroom_milli: Option<u32>,
}

/// Fold one position's per-task results into the rung's per-task tally (first-seen order,
/// same ordering rule as `merge_pos_into_trace`).
fn merge_pos_into_tally(tally: &mut Vec<TaskTally>, pos: &[PosTrace], cap: u32) {
    for pt in pos {
        let fail_cap_hit = (!pt.passed && pt.cap_hit == Some(true)) as u32;
        // Headroom (‰ of the cap unused) for a PASSING cell with a reported count.
        let pass_headroom = match (pt.passed, pt.decoded) {
            (true, Some(d)) if cap > 0 => Some((cap.saturating_sub(d) as u64 * 1000 / cap as u64) as u32),
            _ => None,
        };
        match tally.iter_mut().find(|t| t.task_id == pt.task_id) {
            Some(t) => {
                t.trials += 1;
                t.passed += pt.passed as u32;
                t.failed_cap_hits += fail_cap_hit;
                t.min_pass_headroom_milli = match (t.min_pass_headroom_milli, pass_headroom) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
            }
            None => tally.push(TaskTally {
                task_id: pt.task_id.clone(),
                passed: pt.passed as u32,
                trials: 1,
                failed_cap_hits: fail_cap_hit,
                min_pass_headroom_milli: pass_headroom,
            }),
        }
    }
}

/// Char-safe truncation: keep the first `max` chars, append `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// Keep the head and tail when a string exceeds `max`, eliding the middle — so a needle
/// injected at the FRONT or BACK of the padding stays visible even when the padded prompt
/// dwarfs the cap. Used for the padded user prompt, which is intentionally enormous.
fn truncate_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let half = max / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = s.chars().skip(n - half).collect();
    format!("{head}\n…[{} chars omitted]…\n{tail}", n - 2 * half)
}

/// Fold a position's per-task generations into the rung's grouped trace: append each
/// output under its task (preserving first-seen order), setting the system prompt once.
fn merge_pos_into_trace(trace: &mut Vec<TaskTrace>, depth: f32, pos: Vec<PosTrace>) {
    for pt in pos {
        let out = TraceOutput { depth, prompt: pt.prompt, output: pt.output, passed: pt.passed, decoded: pt.decoded, thinking: pt.thinking, cap_hit: pt.cap_hit };
        match trace.iter_mut().find(|t| t.task_id == pt.task_id) {
            Some(t) => t.outputs.push(out),
            None => trace.push(TaskTrace { task_id: pt.task_id, outputs: vec![out] }),
        }
    }
}

/// Fine-grained progress WITHIN a rung, emitted after each individual task generation.
/// The per-rung `on_rung` seam only fires once a whole rung finishes — at depth a single
/// rung is `positions × tasks` slow model calls and can take minutes, so a UI driven by
/// `on_rung` alone freezes at "rung 1/N" and reads as stuck. This carries enough context
/// for the panel to show live movement (rung, needle position, task) and estimate time.
#[derive(Clone, Copy, Debug)]
pub struct StepProgress {
    /// 1-based current rung and the ladder length.
    pub rung: usize,
    pub total_rungs: usize,
    /// The token depth this rung is padding toward (requested target, pre-verification).
    pub target_tokens: u32,
    /// 1-based needle position within the rung, and how many positions it sweeps.
    pub position: usize,
    pub total_positions: usize,
    /// 1-based task just completed at this position, and the task count.
    pub task: usize,
    pub total_tasks: usize,
}

/// The fine-grained progress seam (see [`StepProgress`]). Boxed `dyn` so the generic
/// engine functions thread one shared sink without each becoming generic over a closure.
type StepSink<'a> = &'a mut (dyn FnMut(StepProgress) + Send);

/// A no-op step sink — used by [`run_cliff`] and tests that don't observe sub-rung steps.
fn no_step(_: StepProgress) {}

/// One needle position's Bernoulli tally, so a rung can POOL its positions instead of taking
/// their minimum. `passed`/`trials` are raw counts precisely so they can be summed — a mean of
/// per-position ratios would silently re-quantize at 1/positions.
#[derive(Clone, Copy, Debug)]
struct PosTally {
    passed: u32,
    trials: u32,
}

/// One needle position within a rung: the composite there and the verified depth.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DepthScore {
    pub depth: f32,
    pub composite: Option<f64>,
    pub verified_tokens: u32,
}

/// One rung of the ladder: what depth was requested, the depth actually verified
/// from `prompt_eval_count`, the worst-position composite, and the per-position
/// breakdown. `composite`/`verified_tokens` are always the MEASURED values.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CliffPoint {
    pub target_tokens: u32,
    pub verified_tokens: u32,
    /// The rung's score, POOLED across the swept positions (`passed / trials`) — not the
    /// worst of them. Worst-of-positions re-quantized the score at 1/tasks (0.2 on the
    /// default 5-task collection = exactly `COLLAPSE_MARGIN`, so one flip read as a cliff)
    /// and compared a min-of-3 against a single-position baseline. `per_depth` still carries
    /// each position, so the weakest spot stays visible even though the verdict pools.
    /// Falls back to the worst position for a mixed/single-turn rung, whose `aggregate()`
    /// cascade has no summable denominator.
    pub composite: Option<f64>,
    /// The rung's raw tally — `Some` only for a purely-agentic rung (see `PosTally`).
    /// Carried so the UI can show the sample size ("12 / 15") and so `classify` can refuse a
    /// verdict the sample can't support, rather than inferring either from `DEFAULT_DEPTHS`:
    /// "never report a number you didn't measure" applies to the sample size itself.
    pub passed: Option<u32>,
    pub trials: Option<u32>,
    pub per_depth: Vec<DepthScore>,
    /// Per-task trace (system prompt + per-position outputs) for THIS rung — every task,
    /// pass or fail (capped). Powers the per-step "View trace" in the UI.
    pub trace: Vec<TaskTrace>,
    /// Per-task pass counts for THIS rung (uncapped — see `TaskTally`). What lets the
    /// verdict and the reader tell a broad collapse from one task breaking. `default` so
    /// pre-field serialized reports still parse.
    #[serde(default)]
    pub by_task: Vec<TaskTally>,
    /// The output-token cap every cell of this rung ran under (answer floor + any
    /// depth-banded scratchpad) — the denominator of every headroom figure. `default`
    /// 0 = an old record that never measured it.
    #[serde(default)]
    pub max_output: u32,
    /// Failing cells that died AT the cap (`finish == "length"`), rung-wide. THE third
    /// bucket: these cells never enter a numerator or denominator that claims to
    /// measure the model — when this is non-zero, `composite` is `None` and every
    /// surface prints the passed / failed / died-at-cap triple instead of a rate
    /// somebody had to define (dropping them overstates, folding them in understates).
    #[serde(default)]
    pub cap_deaths: u32,
}

/// The probe result: every rung, the classified status (mirrors the persisted
/// `CliffStatus`), and `cliff_tokens` — the largest VERIFIED context where the
/// task still passed across all positions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CliffReport {
    pub points: Vec<CliffPoint>,
    pub status: CliffStatus,
    pub cliff_tokens: Option<u32>,
    /// The thinking-budget preset this probe ran under — `Some` only for a thinking
    /// run, so a depth measured with a scratchpad is never conflated with one without
    /// (metric comparability: the mode flag rides with the number). Old serialized
    /// reports (pre-preset) deserialize to `None`, which is exactly what they ran as.
    #[serde(default)]
    pub think_preset: Option<ThinkPreset>,
    /// The decoding temperature the probe actually ran at, stamped by the command/CLI
    /// layer (the engine never decides it). `Some(0.0)` = greedy (the default);
    /// anything else came from the user's global params — carried so a sampled depth
    /// is never conflated with a greedy one. `None` only on pre-field reports.
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// Did `task` FAIL this rung by the cliff's yardstick? A single-turn task must be fully
/// correct (`verdict_passed`). An **agentic** task carries only a placeholder `expected`
/// (its real criterion is the multi-turn `agentic.end_state`), so the cliff ignores that
/// and fails it only when the model emitted **no well-formed tool call** (`!verdict.parsed`)
/// — tool/arg correctness is the end-state's job, not the probe's.
fn cliff_failed(task: &ToolTask, v: &Verdict) -> bool {
    // Agentic tasks (incl. `agent_loop` / MCP) carry a placeholder `expected` the single-turn
    // scorer would misread — the cliff signal for them is well-formedness (a parseable call
    // survived this depth), not full task completion (too slow to run per rung/position).
    if is_agentic(&task.category) {
        !v.parsed
    } else {
        !verdict_passed(v)
    }
}

/// Score one swept position the way the CLIFF needs it. Single-turn tasks keep the full
/// cascaded tool-call composite (`aggregate`); agentic tasks — whose placeholder
/// `expected` the single-turn scorer would mis-read as a forced abstention — are scored on
/// JSON **well-formedness** alone: the fraction that emitted a parseable tool call at this
/// depth. The position composite blends the two groups by task count (both in [0,1]);
/// prompt tokens average over EVERY task, since the x-axis depth is category-blind.
fn cliff_score(tasks: &[ToolTask], results: &[TaskResult]) -> (Option<f64>, Option<f64>, Option<PosTally>) {
    let mut single_tasks: Vec<ToolTask> = Vec::new();
    let mut single_results: Vec<TaskResult> = Vec::new();
    let (mut agentic_parsed, mut agentic_n) = (0usize, 0usize);
    for (t, r) in tasks.iter().zip(results) {
        if is_agentic(&t.category) {
            agentic_n += 1;
            if r.verdict.parsed {
                agentic_parsed += 1;
            }
        } else {
            single_tasks.push(t.clone());
            single_results.push(r.clone());
        }
    }
    // The poolable tally — `Some` ONLY for a purely-agentic rung, where the score is a real
    // Bernoulli count (`parsed / n`) and positions can therefore be summed. A single-turn task
    // scores through `aggregate()`, a graded cascade of conditional sub-rates whose resolution
    // is NOT a simple k/n; pooling those by counting would invent a denominator. Mixed and
    // single-turn rungs keep the documented worst-of-positions instead (see `sweep`).
    let tally = (agentic_n > 0 && single_tasks.is_empty())
        .then(|| PosTally { passed: agentic_parsed as u32, trials: agentic_n as u32 });
    let single_comp = (!single_tasks.is_empty()).then(|| aggregate(&single_tasks, single_results).composite).flatten();
    let agentic_comp = (agentic_n > 0).then(|| agentic_parsed as f64 / agentic_n as f64);
    let composite = match (single_comp, agentic_comp) {
        (Some(s), Some(a)) => {
            let (sn, an) = (single_tasks.len() as f64, agentic_n as f64);
            Some((s * sn + a * an) / (sn + an))
        }
        (Some(s), None) => Some(s),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    };
    let toks: Vec<u32> = results.iter().filter_map(|r| r.prompt_tokens).collect();
    let prompt_tokens = (!toks.is_empty()).then(|| toks.iter().map(|&t| t as f64).sum::<f64>() / toks.len() as f64);
    (composite, prompt_tokens, tally)
}

/// The TRUE prompt size for one turn. llama.cpp serves a reused prefix from its prompt
/// cache and reports only the RECOMPUTED part in `prompt_eval_count`, so the cached prefix
/// (`cache_n`) has to be added back — the same occupancy the agentic runner already trusts
/// (see `attribute_guard`). The cliff sweeps near-identical prompts (same padding, needle
/// moved), so it hits that cache constantly: measured live, a fully-cached 2457-token prompt
/// reports `prompt_n = 1`. Reading that as the depth breaks the probe twice over — the
/// charted/persisted depth collapses toward zero, AND the learned byte→token rate explodes
/// (`bytes / 1`), sizing the next rung far past the window, which llama.cpp rejects outright
/// (killing the whole run). Ollama sends no `cache_n`, so there this is a no-op.
fn occupancy(stats: &GenerateStats) -> Option<u32> {
    stats.prompt_eval_count.map(|p| p.saturating_add(stats.cache_n.unwrap_or(0)))
}

/// Run all tasks at one padding + one needle depth, returning each task's verdict
/// and measured prompt tokens. Empty padding ⇒ the unpadded baseline. `max_output`
/// is this rung's per-turn output cap (answer floor + any depth-banded scratchpad).
#[allow(clippy::too_many_arguments)]
async fn run_position<T: ModelTurn, F: Fn(&ToolTask) -> T>(
    make_turn: &F,
    model: &str,
    tasks: &[ToolTask],
    padding: &str,
    depth: f32,
    max_output: u32,
    on_task: &mut (dyn FnMut(usize, usize) + Send),
) -> AppResult<(Vec<TaskResult>, Vec<PosTrace>)> {
    let mut results = Vec::with_capacity(tasks.len());
    let mut traces = Vec::with_capacity(tasks.len());
    for (ti, task) in tasks.iter().enumerate() {
        let prompt = if padding.is_empty() {
            task.prompt.clone()
        } else {
            inject_at_depth(padding, &task.prompt, depth)
        };
        let system = build_system_for(&task.tools, TerminalGuidance::PlainTextOk);
        let spec = GenerateSpec {
            model: model.to_string(),
            prompt,
            system: Some(system.clone()),
            // Greedy (temp 0) — a probe is a diagnostic and must reproduce. The live
            // command's BackendTurn carries num_ctx (so the padding isn't truncated);
            // this temp 0 is the seam fallback the scripted test model also sees. The
            // anti-collapse repeat_penalty matches the other eval paths so the cliff
            // metric stays comparable; a header value still overrides it.
            options: Some(GenerateOptions { temperature: Some(0.0), repeat_penalty: Some(EVAL_REPEAT_PENALTY), num_predict: Some(max_output), ..Default::default() }),
            keep_alive: None,
            think: None,
        };
        // Per-task turn: the prompt path reuses one shared `&BackendTurn` (the factory ignores
        // the task); the native path builds a fresh `NativeToolTurn` carrying THIS task's tool
        // schemas. A native turn ignores the prompt-based `system` above and builds its own, so
        // scoring stays byte-identical across the two paths.
        let turn = make_turn(task);
        let (raw, stats) = turn.run(&spec, &Progress::new()).await?;
        let cap_hit = stats.finish_reason.as_deref().map(|r| r == "length");
        // A truncated generation (`finish == "length"`) can never PASS, regardless of
        // what parses out of the fragment: the cap censors whatever came next (a wrong
        // follow-up call, a self-correction), so a key match on the fragment is biased
        // toward pass. Gated HERE, before the answer-key match — the mirror of the
        // failing-cap-hit rule, which already refuses to count those cells as model
        // failures. Both land in the same died-at-cap bucket: the cause is the harness
        // cap, not the model. An unreported finish scores normally — absence of
        // measurement is never an attribution.
        let verdict = if cap_hit == Some(true) {
            Verdict { parsed: false, tool_match: false, args_match: false, abstain_correct: None }
        } else {
            score(&task.expected, extract_calls(&raw).as_deref())
        };
        // Keep the padded input + raw completion for EVERY task (pass or fail), so the
        // rung's "View trace" shows exactly what the model saw and emitted at this step.
        // `passed` is the cliff yardstick (`cliff_failed`). The system prompt is the same
        // boilerplate each turn, so it's intentionally not retained.
        traces.push(PosTrace {
            task_id: task.id.clone(),
            prompt: truncate_middle(&spec.prompt, MAX_PROMPT_CHARS),
            output: truncate(&raw, MAX_OUTPUT_CHARS),
            passed: !cliff_failed(task, &verdict),
            decoded: stats.eval_count,
            thinking: stats.thinking_tokens,
            cap_hit,
        });
        results.push(TaskResult {
            id: task.id.clone(),
            category: task.category.clone(),
            verdict,
            // The depth is the context the model READ, not the prefill work the backend did.
            prompt_tokens: occupancy(&stats),
        });
        // Report this task as done so the UI advances during the slow per-rung sweep.
        on_task(ti + 1, tasks.len());
    }
    Ok((results, traces))
}

/// Sweep every needle depth for one fixed padding. Returns the per-depth scores,
/// the mean verified token depth, and the worst-position composite.
#[allow(clippy::too_many_arguments)]
async fn sweep<T: ModelTurn, F: Fn(&ToolTask) -> T>(
    make_turn: &F,
    model: &str,
    tasks: &[ToolTask],
    padding: &str,
    depths: &[f32],
    rung: usize,
    total_rungs: usize,
    target: u32,
    max_output: u32,
    on_step: StepSink<'_>,
) -> AppResult<(Vec<DepthScore>, u32, Option<f64>, Option<PosTally>, Vec<TaskTrace>, Vec<TaskTally>)> {
    let mut per_depth = Vec::with_capacity(depths.len());
    let mut tok_sum: u64 = 0;
    let mut tok_n: u64 = 0;
    let mut worst: Option<f64> = None;
    let mut pooled = PosTally { passed: 0, trials: 0 };
    let mut poolable = true;
    let mut trace: Vec<TaskTrace> = Vec::new();
    let mut by_task: Vec<TaskTally> = Vec::new();
    for (pi, &depth) in depths.iter().enumerate() {
        // Wrap the per-task tick with this position's context so the panel can render
        // "rung r/N · position p/3 · task t/M" and weight an overall completion fraction.
        let total_positions = depths.len();
        let mut on_task = |task: usize, total_tasks: usize| {
            on_step(StepProgress { rung, total_rungs, target_tokens: target, position: pi + 1, total_positions, task, total_tasks });
        };
        let (results, pos_traces) = run_position(make_turn, model, tasks, padding, depth, max_output, &mut on_task).await?;
        let (composite, prompt_tokens, tally) = cliff_score(tasks, &results);
        let vt = prompt_tokens.map(|t| t.round() as u32).unwrap_or(0);
        if let Some(t) = prompt_tokens {
            tok_sum += t.round() as u64;
            tok_n += 1;
        }
        if let Some(c) = composite {
            worst = Some(worst.map_or(c, |w: f64| w.min(c)));
        }
        match tally {
            Some(t) => {
                pooled.passed += t.passed;
                pooled.trials += t.trials;
            }
            // One un-poolable position makes the whole rung un-poolable: a mixed/single-turn
            // rung has no common denominator to sum.
            None => poolable = false,
        }
        per_depth.push(DepthScore { depth, composite, verified_tokens: vt });
        merge_pos_into_tally(&mut by_task, &pos_traces, max_output);
        merge_pos_into_trace(&mut trace, depth, pos_traces);
    }
    trace.truncate(MAX_TRACE_TASKS);
    let mean_tokens = if tok_n > 0 { (tok_sum / tok_n) as u32 } else { 0 };
    // POOL the positions rather than taking their worst. Two defects die here:
    //
    // 1. Resolution. Per position the score is `parsed/n` — for the default 5-task
    //    collection that quantum is 0.2, EXACTLY `COLLAPSE_MARGIN`, so one task flipping
    //    cleared the bar and was reported as a cliff. Pooling makes the denominator
    //    `tasks × positions` (5 → 15, quantum 0.067), and the samples are already being
    //    taken — `min()` was throwing them away. Zero extra model calls.
    // 2. Bias. The baseline rung is measured at ONE position (see `probe_rung`), while a
    //    padded rung was the MIN of three. `E[min of 3] < E[single]`, so comparing them
    //    tilted the verdict toward "cliff" independent of the quantum.
    //
    // The weakest-position semantics survive: a model failing EVERY task at one position
    // pools to 10/15 = 0.667, a 0.333 drop — always ≥ the 0.2 margin, for any n. So a
    // systematic positional failure is still caught; only the sporadic single flip is not.
    // `per_depth` still carries each position, so the UI shows WHICH position broke.
    let tally = (poolable && pooled.trials > 0).then_some(pooled);
    let composite = match tally {
        Some(t) => Some(t.passed as f64 / t.trials as f64),
        // Mixed / single-turn: `aggregate()` is a graded cascade, not a k/n count, so its
        // positions cannot be summed. Keep the documented worst-of-positions there.
        None => worst,
    };
    Ok((per_depth, mean_tokens, composite, tally, trace, by_task))
}

/// A rung is MEASURED only when the padded prompt plus its reply budget fit inside the
/// context window the backend actually gave us. Past that bound a backend does not fail
/// loudly — Ollama truncates the prompt to fit, which silently deletes the injected needle
/// and pins `prompt_eval_count` at the window. Both halves of the rung are then artifacts:
/// the score (the model never saw the task) and the depth (a saturated counter that reads
/// the same no matter how much padding is sent). Such a rung must never be scored, plotted,
/// or persisted as a cliff. The command layer already refuses an over-deep request up front
/// (`cliff_window_gate`); this is the last line, for a model whose declared window is wrong.
fn measurable(mean_tokens: u32, ctx_limit: u32, max_output: u32) -> bool {
    mean_tokens > 0 && mean_tokens.saturating_add(max_output) <= ctx_limit
}

/// Clamp a padding byte-size to what the context window can actually hold, at the known
/// bytes-per-token `rate`. The ladder targets are already inside the window, but the
/// verify-and-adjust REBUILD scales by a measured count and can overshoot — and an oversized
/// prompt is fatal, not approximate: llama.cpp rejects the request ("the prompt is larger
/// than the context window") and the whole probe dies, while Ollama truncates in silence.
/// `NO_CTX_LIMIT` leaves the size untouched. Pure, so the bound is unit-tested directly.
fn cap_bytes(bytes: usize, rate: f64, ctx_limit: u32, max_output: u32) -> usize {
    if ctx_limit == NO_CTX_LIMIT || rate <= 0.0 {
        return bytes;
    }
    // Leave room for the reply on top of the prompt, mirroring `measurable`.
    let ceiling_tokens = ctx_limit.saturating_sub(max_output) as f64;
    bytes.min((ceiling_tokens * rate).round() as usize)
}

/// Probe one rung: build padding for `target` tokens, verify the measured depth is
/// within ±5%, rebuilding proportionally up to `MAX_ADJUST_ATTEMPTS` times, then
/// report the rung at its VERIFIED token count (never the requested one).
///
/// `rate` is the learned bytes-per-token for this (model, source): seeded from it so
/// each rung lands within tolerance on the FIRST sweep, and updated from this rung's
/// own measurement. That turns verify-and-adjust from "re-sweep until close" into one
/// sweep per rung in the common case — the main speed win.
#[allow(clippy::too_many_arguments)]
async fn probe_rung<T: ModelTurn, F: Fn(&ToolTask) -> T>(
    make_turn: &F,
    model: &str,
    tasks: &[ToolTask],
    source_text: &str,
    target: u32,
    depths: &[f32],
    ctx_limit: u32,
    budget: CliffBudget,
    rate: &mut Option<f64>,
    rung: usize,
    total_rungs: usize,
    on_step: StepSink<'_>,
) -> AppResult<CliffPoint> {
    // This rung's per-turn output cap: the answer floor, plus — for a thinking run —
    // the scratchpad banded to THIS rung's depth (deeper context ⇒ bigger budget).
    let max_output = budget.max_output_for(target);
    if target == 0 {
        // Baseline: unpadded, single position.
        let mut on_task = |task: usize, total_tasks: usize| {
            on_step(StepProgress { rung, total_rungs, target_tokens: 0, position: 1, total_positions: 1, task, total_tasks });
        };
        let (results, pos_traces) = run_position(make_turn, model, tasks, "", 0.0, max_output, &mut on_task).await?;
        let (composite, prompt_tokens, tally) = cliff_score(tasks, &results);
        let vt = prompt_tokens.map(|t| t.round() as u32).unwrap_or(0);
        let mut by_task: Vec<TaskTally> = Vec::new();
        merge_pos_into_tally(&mut by_task, &pos_traces, max_output);
        let mut trace: Vec<TaskTrace> = Vec::new();
        merge_pos_into_trace(&mut trace, 0.0, pos_traces);
        trace.truncate(MAX_TRACE_TASKS);
        let cap_deaths: u32 = by_task.iter().map(|t| t.failed_cap_hits).sum();
        // Three-bucket invariant: a POOLABLE rung with cap-deaths has NO single rate —
        // one number either drops those cells (overstates) or folds them (understates).
        // Non-poolable (mixed/single-turn) rungs keep their graded composite: a cascade
        // can't net out cap cells (documented limit; the triple still renders).
        let composite = if cap_deaths > 0 && tally.is_some() { None } else { composite };
        return Ok(CliffPoint {
            target_tokens: 0,
            verified_tokens: vt,
            composite,
            passed: tally.map(|t| t.passed),
            trials: tally.map(|t| t.trials),
            per_depth: vec![DepthScore { depth: 0.0, composite, verified_tokens: vt }],
            trace,
            by_task,
            max_output,
            cap_deaths,
        });
    }
    // Seed from the learned rate (accurate) or the 4:1 fallback on the first padded rung,
    // never sizing past what the window can hold (`cap_bytes`).
    let seed_rate = rate.unwrap_or(BYTES_PER_TOKEN as f64);
    let mut bytes = cap_bytes(((target as f64) * seed_rate).round() as usize, seed_rate, ctx_limit, max_output);
    let mut last: Option<(Vec<DepthScore>, u32, Option<f64>, Option<PosTally>, Vec<TaskTrace>, Vec<TaskTally>)> = None;
    for attempt in 0..=MAX_ADJUST_ATTEMPTS {
        let padding = build_padding(source_text, bytes);
        let (per_depth, mean_tokens, worst, tally, trace, by_task) = sweep(make_turn, model, tasks, &padding, depths, rung, total_rungs, target, max_output, on_step).await?;
        let measured_rate = (mean_tokens > 0).then(|| bytes as f64 / mean_tokens as f64);
        if let Some(r) = measured_rate {
            *rate = Some(r); // learn for the next rung
        }
        let off = if target > 0 { (mean_tokens as f64 - target as f64).abs() / target as f64 } else { 0.0 };
        last = Some((per_depth, mean_tokens, worst, tally, trace, by_task));
        if mean_tokens == 0 || off <= ADJUST_TOLERANCE || attempt == MAX_ADJUST_ATTEMPTS {
            break;
        }
        // Rebuild proportionally: scale the byte seed toward the target — but never past the
        // window. An overshoot here is not a survivable miss: llama.cpp REJECTS an oversized
        // prompt outright ("larger than the context window"), which aborts the whole probe,
        // and Ollama truncates it silently. Better a rung slightly under target than no run.
        bytes = cap_bytes(
            ((bytes as f64) * (target as f64) / (mean_tokens as f64)).round() as usize,
            measured_rate.unwrap_or(seed_rate),
            ctx_limit,
            max_output,
        );
    }
    let (per_depth, mean_tokens, worst, tally, trace, by_task) = last.expect("loop runs at least once");
    let cap_deaths: u32 = by_task.iter().map(|t| t.failed_cap_hits).sum();
    // Three-bucket invariant (see the baseline arm): no single rate on a POOLABLE
    // cap-affected rung — the triple is the only reading nobody had to choose.
    let composite = if cap_deaths > 0 && tally.is_some() { None } else { worst };
    Ok(CliffPoint {
        target_tokens: target,
        verified_tokens: mean_tokens,
        composite,
        passed: tally.map(|t| t.passed),
        trials: tally.map(|t| t.trials),
        per_depth,
        trace,
        by_task,
        max_output,
        cap_deaths,
    })
}

/// Classify the ladder into a `CliffStatus` plus `cliff_tokens` (largest verified
/// Did this rung collapse relative to the baseline? THE one source for that question.
///
/// It exists because the rule had drifted into three copies — `classify`, the early-stop
/// in `run_cliff_with_factory`, and a frontend mirror — and a change to any one of them
/// silently desynchronised the others: an early-stop that breaks on a rung `classify` no
/// longer calls a cliff just truncates the ladder, with nothing to show for it.
fn is_collapse(base: f64, c: f64) -> bool {
    c <= base - COLLAPSE_MARGIN
}

/// Can this rung's sample resolve `COLLAPSE_MARGIN` at all? The score moves in steps of
/// `1/trials`, so when a single sample flipping is worth a whole margin, "collapsed" and
/// "held" are the same measurement — the instrument's resolution IS the detection threshold.
/// That is exactly what shipped: the default 5-task collection scored `parsed/5` per position,
/// quantum 0.2 == `COLLAPSE_MARGIN`, so one task flip was reported as a cliff.
///
/// Pooling the positions (see `sweep`) fixes this for every real collection — 5 tasks × 3
/// positions = 15 trials, quantum 0.067 — leaving only the genuinely unresolvable case
/// (`tasks == 1`, i.e. 3 trials → 0.333 > 0.2), where we refuse rather than guess.
/// `None` trials = a mixed/single-turn rung whose quantum isn't derived: don't gate on what
/// we haven't established.
/// STRICTLY less-than is load-bearing: at `1/trials == COLLAPSE_MARGIN` a single flip is
/// worth exactly one margin and `is_collapse`'s `<=` fires on it. That equality IS the
/// shipped bug (5 tasks → 0.2 == 0.2), so it must fall on the "cannot resolve" side.
fn can_resolve_margin(trials: Option<u32>) -> bool {
    match trials {
        Some(0) | None => true, // nothing measured, or not a countable sample — other arms decide
        Some(t) => (1.0 / t as f64) < COLLAPSE_MARGIN,
    }
}

/// Classify the ladder into a `CliffStatus` plus `cliff_tokens` (largest verified
/// context that still passed across positions). No baseline ⇒ Broken; first rung that
/// drops `COLLAPSE_MARGIN` below the baseline (`is_collapse`) ⇒ Collapsed at that depth;
/// otherwise NoCliff.
fn classify(points: &[CliffPoint]) -> (CliffStatus, Option<u32>) {
    let Some(base) = points.first() else {
        return (CliffStatus::NotProbed, None);
    };
    // A baseline failing purely at the output cap is budget-bound, not broken —
    // checked first, same rule as the per-rung check below.
    // Judged on the FOLDED rate: "is the budget damage verdict-scale?" is a question
    // about the whole rung, cap cells included — while Broken below stays a CONTENT
    // question. A baseline that is only unhealthy because of cap deaths is budget-
    // bound, never broken.
    if let Some(bl) = budget_limited(base) {
        if folded_rate(base).map_or(true, |c| c < BASELINE_PASS) {
            return (bl, None);
        }
    }
    // Health and collapse are judged on the CONTENT rate — cap-death cells measure the
    // harness budget, not the model, so they enter neither numerator nor denominator
    // (the three-bucket invariant). A baseline with a cap death but healthy content
    // proceeds, anchored on its content counts; one with NO content-measurable cells
    // was already handled by `budget_limited` above.
    match content_rate(base) {
        // Can't establish a baseline (no signal, or below the floor) — a cliff number
        // here would be a fabrication, so report Broken and no cliff token.
        //
        // Deliberately decided BEFORE the resolution gate below: `Broken` tests the baseline
        // against `BASELINE_PASS`, not against `COLLAPSE_MARGIN`. Even a 1-task probe resolves
        // that cleanly (its score is 0 or 1; both sit unambiguously off the 0.5 floor), so a
        // coarse sample is no reason to withhold it. Only the collapse comparison needs
        // margin-sized resolution.
        None => return (CliffStatus::Broken { tested: base.verified_tokens }, None),
        Some(c) if c < BASELINE_PASS => return (CliffStatus::Broken { tested: base.verified_tokens }, None),
        Some(_) => {}
    }
    // A healthy baseline that only passed by grazing the cap can't anchor a ladder:
    // every deeper rung would measure the budget, not the model. Refused here (and at
    // rung 0 in the run loop, same `cap_marginal` source) — a padded-rung comparison
    // against an edge-sitting baseline is the confound this gate exists to prevent.
    if let Some(used_milli) = cap_marginal(base) {
        return (CliffStatus::CapMarginal { cap: base.max_output, used_milli }, None);
    }
    // The baseline is healthy, so the remaining question is "did it collapse?" — and THAT is
    // measured against `COLLAPSE_MARGIN`. If one flipped sample is worth a whole margin,
    // "collapsed" and "held" are the same measurement, so neither may be claimed. Read the
    // count off the MEASURED rung (the deepest one carries the pooled `tasks × positions`
    // sample); never infer it from `DEFAULT_DEPTHS`.
    let deepest_trials = points.iter().rev().find_map(|p| p.trials);
    if !can_resolve_margin(deepest_trials) {
        return (CliffStatus::Inconclusive { trials: deepest_trials.unwrap_or(0) }, None);
    }
    let base_comp = content_rate(base).expect("checked Some above");
    let mut largest_pass = base.verified_tokens;
    for p in &points[1..] {
        // Pure-cap rung whose FOLDED rate crosses the margin: a verdict-scale budget
        // event — BudgetLimited, decided before any model claim (the mis-attribution
        // this feature exists to prevent). Gated on the length signal, never content.
        if budget_limited(p).is_some() && folded_rate(p).is_some_and(|f| is_collapse(base_comp, f)) {
            return (budget_limited(p).expect("checked above"), None);
        }
        // The collapse claim must survive on CONTENT failures alone: a mixed rung that
        // only crosses the margin when its cap-deaths are folded in does not collapse —
        // it stays visible as the passed/failed/died-at-cap triple and the ladder
        // continues (deeper rungs may still show a real content collapse).
        if let Some(c) = content_rate(p) {
            if is_collapse(base_comp, c) {
                // Statistical gate on top of the point margin: the drop's Newcombe interval
                // must exclude zero, or the sample can't tell this collapse from noise —
                // a margin-sized drop off a tiny baseline (e.g. 5 single-position trials)
                // is a coin flip, not a finding (Bowyer et al., ICML 2025). Applies only
                // when BOTH ends carry poolable k/n tallies; a mixed/single-turn rung has
                // no summable counts, so it keeps the point-margin rule (documented limit).
                let gated = match (content_counts(base), content_counts(p)) {
                    (Some((bp, bn)), Some((rp, rn))) => {
                        stats::newcombe_drop_interval(bp, bn, rp, rn).is_some_and(|d| d.lo > 0.0)
                    }
                    _ => true,
                };
                if !gated {
                    return (CliffStatus::Inconclusive { trials: p.trials.unwrap_or(0) }, None);
                }
                let concentration = concentration_for(base, p);
                return (CliffStatus::Collapsed { depth: p.verified_tokens, concentration }, Some(largest_pass));
            }
            largest_pass = p.verified_tokens;
        }
    }
    let tested = points.last().map(|p| p.verified_tokens).unwrap_or(base.verified_tokens);
    (CliffStatus::NoCliff { tested }, Some(largest_pass))
}

/// The MODEL-measuring rate of a point: cap-death cells are excluded from BOTH the
/// numerator and the denominator (they measure the harness budget, not the model).
/// Poolable rungs compute it from counts; a mixed/single-turn rung falls back to its
/// graded composite (its cells' cap info can't be netted out of a cascade — documented
/// limit). `None` = nothing content-measurable (e.g. every cell died at the cap).
fn content_rate(p: &CliffPoint) -> Option<f64> {
    match (p.passed, p.trials) {
        (Some(pass), Some(tr)) => {
            let content_trials = tr.saturating_sub(cap_deaths_of(p));
            (content_trials > 0).then(|| pass as f64 / content_trials as f64)
        }
        _ => p.composite,
    }
}

/// The FOLDED rate — cap-deaths counted as failures. Never a model claim (that's
/// `content_rate`); used only to judge whether a budget event is VERDICT-SCALE
/// (a pure-cap rung whose folded rate crosses the margin is BudgetLimited).
fn folded_rate(p: &CliffPoint) -> Option<f64> {
    match (p.passed, p.trials) {
        (Some(pass), Some(tr)) if tr > 0 => Some(pass as f64 / tr as f64),
        _ => p.composite,
    }
}

/// THE one source for a point's cap-death count: the per-task tallies. The serialized
/// `cap_deaths` field is stamped FROM this sum for the wire; verdict math never reads
/// the field, so a hand-built or historical point can't disagree with itself.
fn cap_deaths_of(p: &CliffPoint) -> u32 {
    p.by_task.iter().map(|t| t.failed_cap_hits).sum()
}

/// A point's (passed, content_trials) for interval math — cap-deaths excluded, same
/// rule as `content_rate`. `None` when the point has no poolable counts.
fn content_counts(p: &CliffPoint) -> Option<(u32, u32)> {
    match (p.passed, p.trials) {
        (Some(pass), Some(tr)) => {
            let content_trials = tr.saturating_sub(cap_deaths_of(p));
            (content_trials > 0).then_some((pass, content_trials))
        }
        _ => None,
    }
}

/// A rung is budget-limited when it HAS failures and every one of them stopped at the
/// output cap. Requires cap-hit data on all failing cells: an old/uninstrumented record
/// (no counts) can never claim it — absence of measurement is never an attribution.
fn budget_limited(p: &CliffPoint) -> Option<CliffStatus> {
    let (fails, cap_fails) = p
        .by_task
        .iter()
        .fold((0u32, 0u32), |(f, c), t| (f + (t.trials - t.passed), c + t.failed_cap_hits));
    (fails > 0 && cap_fails == fails).then_some(CliffStatus::BudgetLimited { depth: p.verified_tokens, cap: p.max_output })
}

/// Amber early-warning line: a PASSING cell that used all but this many ‰ of its cap is
/// statistically drifting toward the failure population (incorrect chains run 1.3–2.5×
/// longer; consumption grows with depth). Calibrated for GREEDY decoding — under
/// sampling, within-task length spread is ~3×, so the flag is advisory there.
pub const AMBER_HEADROOM_MILLI: u32 = 150;

/// The baseline cap-headroom gate: a baseline whose tightest PASSING cell used at
/// least this much of the cap (‰) only passed by grazing it — the smallest cap that
/// "passes clean" sits at the edge by construction, so every padded rung would then
/// measure the output budget, not the model (live-proven: a 5/5 baseline at 0‰
/// headroom turned every deeper rung into cap-deaths). Such a run is refused BEFORE
/// any padded rung is paid for. ≥ is load-bearing: "used 0.9 of the cap" rejects.
pub const CAP_MARGINAL_USED_MILLI: u32 = 900;

/// `Some(used_milli)` when the BASELINE passed only by grazing its output cap — the
/// tightest passing cell's used-over-cap is ≥ `CAP_MARGINAL_USED_MILLI`. THE one
/// source for the gate: the run loop stops on it at rung 0 (before any padded rung
/// is paid for) and `classify` returns the verdict from it, so the two can never
/// disagree. Cells that reported no count never fire it — absence of measurement is
/// never an attribution.
fn cap_marginal(p: &CliffPoint) -> Option<u32> {
    let min_headroom = p.by_task.iter().filter_map(|t| t.min_pass_headroom_milli).min()?;
    let used = 1000 - min_headroom.min(1000);
    (used >= CAP_MARGINAL_USED_MILLI).then_some(used)
}

/// Warn when the exact exchangeability p-value says the failure placement is this
/// concentrated under a uniform-failure null less than 10% of the time.
const CONCENTRATION_P_WARN: f64 = 0.10;
/// Stopgap concentration flag independent of the p-value: one task holds ≥ half the
/// rung's failures AND there are enough failures for "half" to mean something.
const CONCENTRATION_MIN_FAILURES: u32 = 3;

/// Failure-concentration evidence for a COLLAPSING rung — `Some` only when the rung's
/// failures cluster in one task (exact p ≤ 0.10, or the ≥50%-of-≥3-failures stopgap).
/// `holds_without` re-runs the FULL collapse rule (point margin + Newcombe gate) with
/// the top task excluded from both the baseline and the rung: `true` means the collapse
/// is not robust to that one task — "depth-general collapse" is not established, which
/// is exactly the over-claim an external review caught in a published run.
fn concentration_for(base: &CliffPoint, rung: &CliffPoint) -> Option<CliffConcentration> {
    // With fewer than two tasks "all failures from one task" is trivially true —
    // concentration is undefined, so no claim (mirrors `concentration_p_value`).
    if rung.by_task.len() < 2 {
        return None;
    }
    let top = rung.by_task.iter().max_by_key(|t| t.trials.saturating_sub(t.passed))?;
    let task_failures = top.trials.saturating_sub(top.passed);
    let total_failures: u32 = rung.by_task.iter().map(|t| t.trials.saturating_sub(t.passed)).sum();
    if task_failures == 0 {
        return None;
    }
    let p = stats::concentration_p_value(&rung.by_task);
    let p_flags = p.is_some_and(|v| v <= CONCENTRATION_P_WARN);
    let stopgap = task_failures * 2 >= total_failures && total_failures >= CONCENTRATION_MIN_FAILURES;
    if !(p_flags || stopgap) {
        return None;
    }
    // Leave-one-task-out: sum the OTHER tasks' tallies at both ends and re-apply the rule.
    let excl = |point: &CliffPoint| -> (u32, u32) {
        point
            .by_task
            .iter()
            .filter(|t| t.task_id != top.task_id)
            .fold((0, 0), |(p0, n0), t| (p0 + t.passed, n0 + t.trials))
    };
    let (bp, bn) = excl(base);
    let (rp, rn) = excl(rung);
    let holds_without = if bn > 0 && rn > 0 {
        let margin = is_collapse(bp as f64 / bn as f64, rp as f64 / rn as f64);
        let gate = stats::newcombe_drop_interval(bp, bn, rp, rn).is_some_and(|d| d.lo > 0.0);
        !(margin && gate)
    } else {
        false // baseline breakdown unavailable — can't re-run the rule, so no claim
    };
    Some(CliffConcentration {
        task_id: top.task_id.clone(),
        task_failures,
        total_failures,
        p_value_milli: (p.unwrap_or(1.0) * 1000.0).round() as u32,
        holds_without,
    })
}

/// Build an ascending token ladder from 0 (the unpadded baseline) up to
/// `max_tokens`, inclusive, across `steps` rungs. `[0, …, max_tokens]`.
pub fn build_ladder(max_tokens: u32, steps: u32) -> Vec<u32> {
    let steps = steps.max(2);
    (0..steps).map(|i| ((max_tokens as u64 * i as u64) / (steps as u64 - 1)) as u32).collect()
}

/// Run the full context-cliff probe: for each token rung (ascending; include 0 for
/// the unpadded baseline), sweep the needle across `depths`, verify the depth, and
/// classify where accuracy collapses. Tauri-free — the command supplies the
/// `ModelTurn` (with num_ctx large enough to fit the deepest rung) and persists the
/// result.
/// `ctx_limit` is [`NO_CTX_LIMIT`] here: the scripted models this wrapper serves have no
/// real context window, so there is nothing to saturate against.
pub async fn run_cliff<M: ModelTurn + Sync>(
    turn: &M,
    model: &str,
    tasks: &[ToolTask],
    source: &CliffSource,
    ladder: &[u32],
    depths: &[f32],
) -> AppResult<CliffReport> {
    run_cliff_with(turn, model, tasks, source, ladder, depths, NO_CTX_LIMIT, CliffBudget::default(), &CancellationToken::new(), &mut |_, _, _| {}, &mut no_step).await
}

/// Same as [`run_cliff`] but invokes `on_rung(done, total, point)` after each rung
/// completes — the seam the command layer uses to emit live progress events while
/// the engine stays UI-free. `on_step` is the finer seam: it fires after EVERY task
/// generation (see [`StepProgress`]) so the UI shows movement DURING a rung, not only
/// when one finishes — a deep rung is minutes of model calls, and per-rung events alone
/// look stuck. `cancel` lets the Stop button abort the sweep: it's checked before each
/// (costly) rung and before classification, so a cancelled probe returns an error
/// WITHOUT classifying or persisting a bogus outcome.
#[allow(clippy::too_many_arguments)]
pub async fn run_cliff_with<M: ModelTurn + Sync>(
    turn: &M,
    model: &str,
    tasks: &[ToolTask],
    source: &CliffSource,
    ladder: &[u32],
    depths: &[f32],
    ctx_limit: u32,
    budget: CliffBudget,
    cancel: &CancellationToken,
    on_rung: &mut (dyn FnMut(usize, usize, &CliffPoint) + Send),
    on_step: StepSink<'_>,
) -> AppResult<CliffReport> {
    // One reused turn flows through the per-task factory seam via the blanket `&M` impl (the
    // factory ignores the task and hands back the shared reference). The native path calls
    // `run_cliff_with_factory` directly with a task-aware factory instead.
    run_cliff_with_factory(&|_: &ToolTask| turn, model, tasks, source, ladder, depths, ctx_limit, budget, cancel, on_rung, on_step).await
}

/// The cliff engine over a per-task turn FACTORY: `make_turn(task)` yields the `ModelTurn` for
/// that task. The prompt path passes a factory that returns one shared `&BackendTurn`; the
/// native path passes one that builds a fresh `NativeToolTurn` carrying the task's tool schemas.
/// Everything downstream (padding, sweeping, scoring, classification) is identical — only the
/// turn construction differs.
#[allow(clippy::too_many_arguments)]
pub async fn run_cliff_with_factory<T: ModelTurn, F: Fn(&ToolTask) -> T>(
    make_turn: &F,
    model: &str,
    tasks: &[ToolTask],
    source: &CliffSource,
    ladder: &[u32],
    depths: &[f32],
    ctx_limit: u32,
    budget: CliffBudget,
    cancel: &CancellationToken,
    on_rung: &mut (dyn FnMut(usize, usize, &CliffPoint) + Send),
    on_step: StepSink<'_>,
) -> AppResult<CliffReport> {
    let source_text = source.text();
    let total = ladder.len();
    let mut points = Vec::with_capacity(total);
    // Learned bytes-per-token, shared across rungs so each sizes on one sweep.
    let mut rate: Option<f64> = None;
    // The baseline (first rung) composite — the plateau a cliff falls off.
    let mut baseline_comp: Option<f64> = None;
    for (i, &target) in ladder.iter().enumerate() {
        // Honour a user Stop: a cancelled token aborts BEFORE the next (costly) rung,
        // so the model stops being called instead of grinding through the whole ladder.
        if cancel.is_cancelled() {
            return Err(AppError::Inference("context-cliff probe cancelled".into()));
        }
        let point = probe_rung(make_turn, model, tasks, source_text, target, depths, ctx_limit, budget, &mut rate, i + 1, total, on_step).await?;
        // A Stop that fired DURING this rung leaves it half-generated (cancelled turns
        // return empty/partial text). Abort before emitting it, so a stopped/superseded
        // run never pushes a garbage rung into the chart or the report.
        if cancel.is_cancelled() {
            return Err(AppError::Inference("context-cliff probe cancelled".into()));
        }
        // The prompt hit the context window: the backend truncated it rather than erroring, so
        // this rung's score and depth are both artifacts (see `measurable`). DROP it — never
        // emit, plot, or classify a rung we didn't actually measure — and stop, since every
        // deeper rung saturates the same way. The ladder keeps the rungs that DID fit, so the
        // verdict is computed only from real measurements (an honest "held to <last measured>"
        // instead of a fabricated cliff at the window).
        if i > 0 && !measurable(point.verified_tokens, ctx_limit, budget.max_output_for(target)) {
            break;
        }
        on_rung(i + 1, total, &point);
        // Early-stop reads the same numbers classify will judge: CONTENT rate for
        // model claims, FOLDED rate for budget-event scale — never a folded model claim.
        let comp = content_rate(&point);
        let folded = folded_rate(&point);
        let pure_cap = budget_limited(&point).is_some();
        points.push(point);

        // Early-stop — skip the slowest deep rungs once the outcome is decided:
        if i == 0 {
            baseline_comp = comp;
            // A baseline that is budget-dead (folded below the floor) or content-broken
            // can't anchor a cliff — stop before paying for any padded rung.
            if folded.map_or(true, |f| f < BASELINE_PASS) || comp.map_or(true, |c| c < BASELINE_PASS) {
                break;
            }
            // Cap-marginal baseline (passed, but only by grazing the cap): every padded
            // rung would measure the budget, not the model — stop before paying for any.
            // Same `cap_marginal` source `classify` reads, so loop and verdict agree.
            if cap_marginal(points.last().expect("baseline just pushed")).is_some() {
                break;
            }
        } else if pure_cap && baseline_comp.zip(folded).is_some_and(|(b, f)| is_collapse(b, f)) {
            // Verdict-scale pure-cap rung: classify returns BudgetLimited here — deeper
            // rungs would only repeat the budget event at higher context cost.
            break;
        } else if let (Some(b), Some(c)) = (baseline_comp, comp) {
            // First collapse IS the cliff (classify takes the first drop); deeper
            // rungs would only re-confirm failure at the highest context cost. Shares
            // `is_collapse` with `classify` so the two can never disagree about which
            // rung ended the run.
            if is_collapse(b, c) {
                break;
            }
        }
    }
    // A Stop during the final rung lands here — abort before classify/persist so the
    // cancelled run never overwrites a real saved cliff status with a half-run one.
    if cancel.is_cancelled() {
        return Err(AppError::Inference("context-cliff probe cancelled".into()));
    }
    let (status, cliff_tokens) = classify(&points);
    // The mode flag rides with the result: `Some(preset)` only when the scratchpad was on.
    let think_preset = budget.is_thinking.then_some(budget.preset);
    // `temperature` is stamped by the command/CLI layer, which owns the decoding config.
    Ok(CliffReport { points, status, cliff_tokens, think_preset, temperature: None })
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

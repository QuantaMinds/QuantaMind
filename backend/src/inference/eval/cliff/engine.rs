use super::padding::{build_padding, inject_at_depth};
use super::presets::CliffSource;
use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::model_turn::ModelTurn;
use crate::inference::eval::readiness::types::CliffStatus;
use crate::inference::eval::toolcall::eval::{aggregate, TaskResult};
use crate::inference::eval::toolcall::parse::extract_calls;
use crate::inference::eval::toolcall::prompt::{build_system_for, TerminalGuidance};
use crate::inference::eval::toolcall::score::{score, verdict_passed, Verdict};
use crate::inference::eval::toolcall::tasks::{is_agentic, ToolTask};
use crate::inference::generate::generate_options::{GenerateOptions, EVAL_REPEAT_PENALTY};
use crate::inference::generate::generate_spec::GenerateSpec;
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
/// Output token cap per probe turn — only a tool call is expected, never prose.
const MAX_OUTPUT: u32 = 256;
/// The baseline rung must clear this composite or the run is `Broken` (the model
/// can't even do the task unpadded — a cliff number would be meaningless).
const BASELINE_PASS: f64 = 0.5;
/// A deeper rung is a cliff when its composite falls this far below the baseline.
const COLLAPSE_MARGIN: f64 = 0.2;
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
        let out = TraceOutput { depth, prompt: pt.prompt, output: pt.output, passed: pt.passed };
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
    /// Worst composite across the swept positions — "passes across positions" means
    /// robust everywhere, so the cliff is found at the weakest spot, not the average.
    pub composite: Option<f64>,
    pub per_depth: Vec<DepthScore>,
    /// Per-task trace (system prompt + per-position outputs) for THIS rung — every task,
    /// pass or fail (capped). Powers the per-step "View trace" in the UI.
    pub trace: Vec<TaskTrace>,
}

/// The probe result: every rung, the classified status (mirrors the persisted
/// `CliffStatus`), and `cliff_tokens` — the largest VERIFIED context where the
/// task still passed across all positions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CliffReport {
    pub points: Vec<CliffPoint>,
    pub status: CliffStatus,
    pub cliff_tokens: Option<u32>,
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
fn cliff_score(tasks: &[ToolTask], results: &[TaskResult]) -> (Option<f64>, Option<f64>) {
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
    (composite, prompt_tokens)
}

/// Run all tasks at one padding + one needle depth, returning each task's verdict
/// and measured prompt tokens. Empty padding ⇒ the unpadded baseline.
async fn run_position<T: ModelTurn, F: Fn(&ToolTask) -> T>(
    make_turn: &F,
    model: &str,
    tasks: &[ToolTask],
    padding: &str,
    depth: f32,
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
            options: Some(GenerateOptions { temperature: Some(0.0), repeat_penalty: Some(EVAL_REPEAT_PENALTY), num_predict: Some(MAX_OUTPUT), ..Default::default() }),
            keep_alive: None,
            think: None,
        };
        // Per-task turn: the prompt path reuses one shared `&BackendTurn` (the factory ignores
        // the task); the native path builds a fresh `NativeToolTurn` carrying THIS task's tool
        // schemas. A native turn ignores the prompt-based `system` above and builds its own, so
        // scoring stays byte-identical across the two paths.
        let turn = make_turn(task);
        let (raw, stats) = turn.run(&spec).await?;
        let verdict = score(&task.expected, extract_calls(&raw).as_deref());
        // Keep the padded input + raw completion for EVERY task (pass or fail), so the
        // rung's "View trace" shows exactly what the model saw and emitted at this step.
        // `passed` is the cliff yardstick (`cliff_failed`). The system prompt is the same
        // boilerplate each turn, so it's intentionally not retained.
        traces.push(PosTrace {
            task_id: task.id.clone(),
            prompt: truncate_middle(&spec.prompt, MAX_PROMPT_CHARS),
            output: truncate(&raw, MAX_OUTPUT_CHARS),
            passed: !cliff_failed(task, &verdict),
        });
        results.push(TaskResult {
            id: task.id.clone(),
            category: task.category.clone(),
            verdict,
            prompt_tokens: stats.prompt_eval_count,
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
    on_step: StepSink<'_>,
) -> AppResult<(Vec<DepthScore>, u32, Option<f64>, Vec<TaskTrace>)> {
    let mut per_depth = Vec::with_capacity(depths.len());
    let mut tok_sum: u64 = 0;
    let mut tok_n: u64 = 0;
    let mut worst: Option<f64> = None;
    let mut trace: Vec<TaskTrace> = Vec::new();
    for (pi, &depth) in depths.iter().enumerate() {
        // Wrap the per-task tick with this position's context so the panel can render
        // "rung r/N · position p/3 · task t/M" and weight an overall completion fraction.
        let total_positions = depths.len();
        let mut on_task = |task: usize, total_tasks: usize| {
            on_step(StepProgress { rung, total_rungs, target_tokens: target, position: pi + 1, total_positions, task, total_tasks });
        };
        let (results, pos_traces) = run_position(make_turn, model, tasks, padding, depth, &mut on_task).await?;
        let (composite, prompt_tokens) = cliff_score(tasks, &results);
        let vt = prompt_tokens.map(|t| t.round() as u32).unwrap_or(0);
        if let Some(t) = prompt_tokens {
            tok_sum += t.round() as u64;
            tok_n += 1;
        }
        if let Some(c) = composite {
            worst = Some(worst.map_or(c, |w: f64| w.min(c)));
        }
        per_depth.push(DepthScore { depth, composite, verified_tokens: vt });
        merge_pos_into_trace(&mut trace, depth, pos_traces);
    }
    trace.truncate(MAX_TRACE_TASKS);
    let mean_tokens = if tok_n > 0 { (tok_sum / tok_n) as u32 } else { 0 };
    Ok((per_depth, mean_tokens, worst, trace))
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
    rate: &mut Option<f64>,
    rung: usize,
    total_rungs: usize,
    on_step: StepSink<'_>,
) -> AppResult<CliffPoint> {
    if target == 0 {
        // Baseline: unpadded, single position.
        let mut on_task = |task: usize, total_tasks: usize| {
            on_step(StepProgress { rung, total_rungs, target_tokens: 0, position: 1, total_positions: 1, task, total_tasks });
        };
        let (results, pos_traces) = run_position(make_turn, model, tasks, "", 0.0, &mut on_task).await?;
        let (composite, prompt_tokens) = cliff_score(tasks, &results);
        let vt = prompt_tokens.map(|t| t.round() as u32).unwrap_or(0);
        let mut trace: Vec<TaskTrace> = Vec::new();
        merge_pos_into_trace(&mut trace, 0.0, pos_traces);
        trace.truncate(MAX_TRACE_TASKS);
        return Ok(CliffPoint {
            target_tokens: 0,
            verified_tokens: vt,
            composite,
            per_depth: vec![DepthScore { depth: 0.0, composite, verified_tokens: vt }],
            trace,
        });
    }
    // Seed from the learned rate (accurate) or the 4:1 fallback on the first padded rung.
    let mut bytes = match *rate {
        Some(r) => ((target as f64) * r).round() as usize,
        None => target as usize * BYTES_PER_TOKEN,
    };
    let mut last: Option<(Vec<DepthScore>, u32, Option<f64>, Vec<TaskTrace>)> = None;
    for attempt in 0..=MAX_ADJUST_ATTEMPTS {
        let padding = build_padding(source_text, bytes);
        let (per_depth, mean_tokens, worst, trace) = sweep(make_turn, model, tasks, &padding, depths, rung, total_rungs, target, on_step).await?;
        if mean_tokens > 0 {
            *rate = Some(bytes as f64 / mean_tokens as f64); // learn for the next rung
        }
        let off = if target > 0 { (mean_tokens as f64 - target as f64).abs() / target as f64 } else { 0.0 };
        last = Some((per_depth, mean_tokens, worst, trace));
        if mean_tokens == 0 || off <= ADJUST_TOLERANCE || attempt == MAX_ADJUST_ATTEMPTS {
            break;
        }
        // Rebuild proportionally: scale the byte seed toward the target.
        bytes = ((bytes as f64) * (target as f64) / (mean_tokens as f64)).round() as usize;
    }
    let (per_depth, mean_tokens, worst, trace) = last.expect("loop runs at least once");
    Ok(CliffPoint { target_tokens: target, verified_tokens: mean_tokens, composite: worst, per_depth, trace })
}

/// Classify the ladder into a `CliffStatus` plus `cliff_tokens` (largest verified
/// context that still passed across positions). Mirrors the frontend
/// `classifyCliff` contract: no baseline ⇒ Broken; first rung that drops
/// `COLLAPSE_MARGIN` below the baseline ⇒ Collapsed at that depth; otherwise NoCliff.
fn classify(points: &[CliffPoint]) -> (CliffStatus, Option<u32>) {
    let Some(base) = points.first() else {
        return (CliffStatus::NotProbed, None);
    };
    match base.composite {
        // Can't establish a baseline (no signal, or below the floor) — a cliff number
        // here would be a fabrication, so report Broken and no cliff token.
        None => return (CliffStatus::Broken { tested: base.verified_tokens }, None),
        Some(c) if c < BASELINE_PASS => return (CliffStatus::Broken { tested: base.verified_tokens }, None),
        Some(_) => {}
    }
    let base_comp = base.composite.expect("checked Some above");
    let mut largest_pass = base.verified_tokens;
    for p in &points[1..] {
        if let Some(c) = p.composite {
            if c <= base_comp - COLLAPSE_MARGIN {
                return (CliffStatus::Collapsed { depth: p.verified_tokens }, Some(largest_pass));
            }
            largest_pass = p.verified_tokens;
        }
    }
    let tested = points.last().map(|p| p.verified_tokens).unwrap_or(base.verified_tokens);
    (CliffStatus::NoCliff { tested }, Some(largest_pass))
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
pub async fn run_cliff<M: ModelTurn + Sync>(
    turn: &M,
    model: &str,
    tasks: &[ToolTask],
    source: &CliffSource,
    ladder: &[u32],
    depths: &[f32],
) -> AppResult<CliffReport> {
    run_cliff_with(turn, model, tasks, source, ladder, depths, &CancellationToken::new(), &mut |_, _, _| {}, &mut no_step).await
}

/// Same as [`run_cliff`] but invokes `on_rung(done, total, point)` after each rung
/// completes — the seam the command layer uses to emit live progress events while
/// the engine stays UI-free. `on_step` is the finer seam: it fires after EVERY task
/// generation (see [`StepProgress`]) so the UI shows movement DURING a rung, not only
/// when one finishes — a deep rung is minutes of model calls, and per-rung events alone
/// look stuck. `cancel` lets the Stop button abort the sweep: it's checked before each
/// (costly) rung and before classification, so a cancelled probe returns an error
/// WITHOUT classifying or persisting a bogus outcome.
pub async fn run_cliff_with<M: ModelTurn + Sync>(
    turn: &M,
    model: &str,
    tasks: &[ToolTask],
    source: &CliffSource,
    ladder: &[u32],
    depths: &[f32],
    cancel: &CancellationToken,
    on_rung: &mut (dyn FnMut(usize, usize, &CliffPoint) + Send),
    on_step: StepSink<'_>,
) -> AppResult<CliffReport> {
    // One reused turn flows through the per-task factory seam via the blanket `&M` impl (the
    // factory ignores the task and hands back the shared reference). The native path calls
    // `run_cliff_with_factory` directly with a task-aware factory instead.
    run_cliff_with_factory(&|_: &ToolTask| turn, model, tasks, source, ladder, depths, cancel, on_rung, on_step).await
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
        let point = probe_rung(make_turn, model, tasks, source_text, target, depths, &mut rate, i + 1, total, on_step).await?;
        // A Stop that fired DURING this rung leaves it half-generated (cancelled turns
        // return empty/partial text). Abort before emitting it, so a stopped/superseded
        // run never pushes a garbage rung into the chart or the report.
        if cancel.is_cancelled() {
            return Err(AppError::Inference("context-cliff probe cancelled".into()));
        }
        on_rung(i + 1, total, &point);
        let comp = point.composite;
        points.push(point);

        // Early-stop — skip the slowest deep rungs once the outcome is decided:
        if i == 0 {
            baseline_comp = comp;
            // A broken / unmeasurable baseline can't have a "cliff" — stop before
            // paying for any padded rung.
            if comp.map_or(true, |c| c < BASELINE_PASS) {
                break;
            }
        } else if let (Some(b), Some(c)) = (baseline_comp, comp) {
            // First collapse IS the cliff (classify takes the first drop); deeper
            // rungs would only re-confirm failure at the highest context cost.
            if c <= b - COLLAPSE_MARGIN {
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
    Ok(CliffReport { points, status, cliff_tokens })
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

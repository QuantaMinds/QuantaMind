use crate::errors::AppResult;
use crate::inference::eval::agentic::context::{tool_result_line, Conversation};
use crate::inference::eval::agentic::env_view::{env_view, EnvView};
use crate::inference::eval::agentic::scoring::endstate;
use crate::inference::eval::agentic::model_turn::ModelTurn;
use crate::inference::eval::agentic::scoring::report::{AgenticReport, FailureKind, RunOutcome};
use crate::inference::eval::agentic::sandbox::{canonical, DeterministicSandbox, EndStateRule, ResponderKind, SandboxState, TaskCheckpoint};
use crate::inference::eval::agentic::v2::env_webui::WebUiState;
use crate::inference::eval::agentic::v2::r#match::text_matches;
use crate::inference::eval::agentic::step::{StepKind, TrajectoryStep};
use crate::inference::eval::toolcall::parse::{
    extract_calls_dialect, is_empty_output, looks_like_broken_json, looks_like_foreign_dialect, strip_think,
    ToolCallDialect,
};
use crate::inference::eval::toolcall::prompt::{build_system_for, TerminalGuidance};
use crate::inference::generate::generate_options::{GenerateOptions, EVAL_REPEAT_PENALTY};
use crate::inference::generate::generate_spec::GenerateSpec;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

const UNKNOWN_TOOL: &str =
    "Tool not found or arguments unrecognized. Choose a tool from the provided schema.";

/// Identical, no-progress turns in a row before the run is declared a loop. A model that
/// re-emits the exact same (tool + args) turn this many times without advancing the
/// end-state is stuck (an ack like `{"ok":true}` gives it no signal to change), so fail it
/// fast as `InfiniteLoop` instead of burning the whole `max_steps` budget — the verdict is
/// the same, just reached in 3 steps instead of up to 85. A turn that DIFFERS or advances a
/// checkpoint resets the counter, so legitimate multi-step progress is never cut short.
const STALL_REPEAT_LIMIT: u32 = 3;

/// The looser stall guard: how many consecutive turns may advance NO checkpoint before the run
/// is failed as `InfiniteLoop`, REGARDLESS of whether the calls varied. `STALL_REPEAT_LIMIT`
/// only catches byte-identical repeats; a "busy loop" that keeps changing its calls slightly
/// (e.g. re-searching the same two symbols every turn) dodges it and burns the whole horizon.
/// Set well above `STALL_REPEAT_LIMIT` so legitimate multi-turn exploration (read→read→write,
/// no checkpoint until the write) is never cut short — only a genuinely stuck agent reaches it.
const STALL_NO_PROGRESS_LIMIT: u32 = 8;

/// How many times a single step may be re-run when the turn was cut off at the `num_predict`
/// cap (`finish_reason == "length"`) and parsed to zero calls. Each retry doubles the budget,
/// clamped to the context headroom left this turn; a re-roll does NOT advance `step_index`, so
/// fitting the output never costs the model a turn of its horizon. Small: 2 doublings (e.g.
/// 3072 → 6144 → 12288) clears any realistic batched tool-call payload, and a turn that still
/// truncates after that is context-bound, labeled honestly as `Truncated`.
const TRUNCATION_RETRY_LIMIT: u32 = 2;

/// `num_ctx` sizing for the agentic loop. The transcript re-sent every step grows by
/// ~one assistant turn + tool result per step; left at the model default (~4096) a
/// multi-step transcript overflows, triggering Ollama context-shift that BOTH busts
/// the automatic prefix-KV cache (full re-prefill every turn — the stall) AND silently
/// drops the earliest turns (the model loses the start of its own run). Size from the
/// step cap so the window covers the worst-case transcript, clamped to a memory-safe
/// ceiling: a 16GB host can't hold the deepest Extreme (85-step ≈ 30k-token) context,
/// so those still shift — a hardware limit, not a regression. Per-host scaling above
/// the ceiling is deferred (would need the hardware class threaded in here).
const NUM_CTX_BASE: u32 = 2048; // system prompt (with decoys) + initial prompt headroom
const NUM_CTX_PER_STEP: u32 = 384; // ≈ assistant turn (≤256) + tool result + formatting
const NUM_CTX_FLOOR: u32 = 4096;
const NUM_CTX_CEILING: u32 = 16384; // memory-safe on a 16GB host; covers Easy→Hard fully

/// Context window for a run of `max_steps` steps: cover the worst-case transcript, but
/// never exceed the memory-safe ceiling. See [`NUM_CTX_CEILING`].
fn agentic_num_ctx(max_steps: u32) -> u32 {
    NUM_CTX_BASE
        .saturating_add(max_steps.saturating_mul(NUM_CTX_PER_STEP))
        .clamp(NUM_CTX_FLOOR, NUM_CTX_CEILING)
}

/// Push the raw model turn to the transcript exactly once per turn, lazily — the
/// first injected result triggers it, so a turn that terminates before any injection
/// (end-state on the first call, a budget-spent schema error) pushes nothing, matching
/// the old single-call terminal path byte-for-byte.
fn ensure_model_pushed(convo: &mut Conversation, raw: &str, pushed: &mut bool) {
    if !*pushed {
        convo.push_model(raw);
        *pushed = true;
    }
}

/// Join a turn's per-call injection lines into the single `TrajectoryStep.injection`
/// the UI renders. `None` when the turn injected nothing (a terminal turn) — so a
/// single-call terminal step still streams `injection: None`, unchanged.
fn join_injection(lines: &[(StepKind, String)]) -> Option<String> {
    (!lines.is_empty()).then(|| lines.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n"))
}

/// Collapse a non-terminal turn's per-call lines into one `(kind, injection)` for the
/// streamed step. A single call keeps its exact kind (so the single-call path is
/// byte-identical to before); a homogeneous multi-call turn keeps the shared kind;
/// a mixed multi-call turn reports `ToolCall` (the turn ran tools), with every result
/// in the joined injection.
fn summarize_turn(lines: &[(StepKind, String)]) -> (StepKind, Option<String>) {
    match lines {
        [] => (StepKind::ToolCall, None),
        [(kind, _), ..] if lines.iter().all(|(k, _)| k == kind) => (kind.clone(), join_injection(lines)),
        _ => (StepKind::ToolCall, join_injection(lines)),
    }
}

/// A checkpoint's reporter text glob, if it is a reporter (carries a `text` string arg).
fn reporter_text(cp: &TaskCheckpoint) -> Option<&str> {
    cp.args.get("text").and_then(|v| v.as_str())
}

/// G3: is a no-call yield a content-correct, wrong-channel answer rather than a true
/// hallucination? True ONLY when EXACTLY ONE checkpoint is unsatisfied, that checkpoint is
/// a reporter (a `text` glob), and the model's prose matches it. The "exactly one" guard is
/// load-bearing: a weak glob like `*3*` must not relabel a model that SKIPPED the work and
/// happened to emit the answer token — requiring every other checkpoint satisfied makes the
/// prose match evidence the model did the task, not a coincidence.
fn reported_in_prose(end_state: &EndStateRule, satisfied: &[bool], next_cp: usize, raw: &str) -> bool {
    let unsatisfied: Vec<&TaskCheckpoint> = match end_state {
        EndStateRule::RequireAll(cps) => cps.iter().zip(satisfied).filter(|(_, &s)| !s).map(|(c, _)| c).collect(),
        EndStateRule::RequireSequence(cps) => cps.get(next_cp..).unwrap_or(&[]).iter().collect(),
        EndStateRule::ExpectAbstainingText | EndStateRule::RequireEndState(_) => return false,
    };
    matches!(unsatisfied.as_slice(), [cp] if reporter_text(cp).is_some_and(|p| text_matches(p, raw)))
}

/// Pass^k inputs: how many independent runs (default 5), the per-run step cap, and
/// the per-run semantic-recovery budget (how many schema errors a run may correct
/// before it's scored MalformedSchema).
pub struct AgenticConfig {
    pub k: u32,
    pub max_steps: u32,
    pub max_recovery: u8,
}

impl Default for AgenticConfig {
    fn default() -> Self {
        Self { k: 5, max_steps: 10, max_recovery: 2 }
    }
}

/// The Pass^k consistency engine: run the agentic loop `k` times and fold the
/// outcomes into an `AgenticReport`. Each `run_once` builds a fresh transcript and
/// token counter over the shared (immutable) sandbox — absolute isolation, no
/// state bleed between iterations.
///
/// A per-run backend error (e.g. Ollama timed out or crashed on one of the k
/// attempts) does NOT abort the batch: that run is skipped and the remaining
/// attempts still execute, then the report folds the runs that completed. An infra
/// fault is not a model task-failure, so a skipped run never reaches the
/// denominator. Only when EVERY run errored does the error propagate — the task
/// then shows as Error and re-runs on resume (the backend is genuinely down).
pub async fn run_agentic<M: ModelTurn>(
    turn: &M,
    sandbox: &DeterministicSandbox,
    config: AgenticConfig,
    tx: &UnboundedSender<TrajectoryStep>,
) -> AppResult<AgenticReport> {
    // Non-generated tasks reuse one sandbox for every run (a constant factory).
    let never = CancellationToken::new();
    run_agentic_with(turn, config.k, |_| Ok((sandbox.clone(), config.max_steps, config.max_recovery)), &never, tx)
        .await
}

/// Pass^k with a per-run sandbox FACTORY. `make(run_index)` returns the sandbox +
/// (max_steps, max_recovery) for that repetition — so a generated task can build a
/// FRESH instance per run (contamination resistance) while a static task returns
/// the same sandbox each time. A factory `Err` (e.g. a generation failure) skips
/// that run like an infra error; only when EVERY run is skipped/errored does the
/// error propagate. The infra-error-skip semantics are otherwise unchanged.
pub async fn run_agentic_with<M, F>(
    turn: &M,
    k: u32,
    make: F,
    cancel: &CancellationToken,
    tx: &UnboundedSender<TrajectoryStep>,
) -> AppResult<AgenticReport>
where
    M: ModelTurn,
    F: Fn(u32) -> AppResult<(DeterministicSandbox, u32, u8)>,
{
    run_agentic_within(turn, k, make, cancel, task_budget(k, turn.is_thinking()), tx).await
}

/// Per-RUN wall-clock allotment, multiplied by `k` to get the whole-batch budget
/// (`task_budget`). A slow model (a 12B on a 16GB host generates minutes per step) can
/// otherwise grind for hours: k runs × max_steps real multi-minute turns. Once a batch passes
/// its budget we stop launching NEW runs and report the honest pass rate over the COMPLETED
/// runs (flagged via `AgenticReport::with_truncation`).
///
/// It MUST scale with `k`: a flat per-task cap silently broke Pass^k for every tier above the
/// smallest — a slow model takes minutes per run, so a flat 8-min cap let only ~3 runs through
/// whether k was 5 or 16, truncating the batch (and voiding its strict-pass credit, see
/// `AgenticReport::is_strict_pass`) before it could ever finish. Per-run scaling gives each
/// requested repetition a guaranteed slice, so the cap only fires on a pathologically slow batch
/// (every run blowing its whole allotment), never merely because k is large. 5 min/run is
/// generous for a terse model — a healthy 7B finishes a run in seconds and never approaches it.
const PER_RUN_BUDGET: std::time::Duration = std::time::Duration::from_secs(300);

/// A reasoning model emits a 1–2k+ token `<think>` scratchpad BEFORE every tool call — the
/// per-turn token budget it gets is already 6–16× a terse model's (`max_tokens_for(.., true)` =
/// 1536–4096 vs 256), so each turn is several times slower in wall-clock, and a run is many turns.
/// Without this a thinking model on a deep task blows the per-run slice and gets truncated even
/// though it's progressing normally. 3× (not the full token ratio) because not every turn maxes
/// its budget and per-turn overhead is partly fixed; `STEP_TIMEOUT` still caps any single wedged turn.
const THINKING_BUDGET_MULTIPLIER: u32 = 3;

/// The whole-batch wall-clock budget for a `k`-run Pass^k task: a per-run slice (larger for a
/// thinking model, which is intrinsically slower per turn) times `k`.
fn task_budget(k: u32, is_thinking: bool) -> std::time::Duration {
    let per_run = if is_thinking { PER_RUN_BUDGET * THINKING_BUDGET_MULTIPLIER } else { PER_RUN_BUDGET };
    per_run * k.max(1)
}

/// `run_agentic_with` with an injectable wall-clock budget (so the truncation path is
/// testable without a multi-minute wait — a ZERO budget truncates after the first run).
async fn run_agentic_within<M, F>(
    turn: &M,
    k: u32,
    make: F,
    cancel: &CancellationToken,
    budget: std::time::Duration,
    tx: &UnboundedSender<TrajectoryStep>,
) -> AppResult<AgenticReport>
where
    M: ModelTurn,
    F: Fn(u32) -> AppResult<(DeterministicSandbox, u32, u8)>,
{
    let start = std::time::Instant::now();
    let mut outcomes = Vec::with_capacity(k as usize);
    let mut last_err = None;
    let mut truncated = false;
    for run_index in 0..k {
        // Halt a long Pass^k task promptly on cancel (the batch loop also checks
        // between tasks; this bounds an interrupt to ≤1 run of a big-k task).
        if cancel.is_cancelled() {
            break;
        }
        // Wall-clock backstop: stop launching runs once the batch blows its budget — but
        // only AFTER one whole run (always sample at least once, even on a slow box) and
        // only BETWEEN runs (never mid-run, so every counted run is complete). The pass
        // rate stays honest over the runs that finished; the report is flagged truncated.
        if run_index > 0 && start.elapsed() >= budget {
            truncated = true;
            break;
        }
        let (sandbox, max_steps, max_recovery) = match make(run_index) {
            Ok(t) => t,
            Err(e) => {
                last_err = Some(e); // a generation failure skips this run
                continue;
            }
        };
        match run_once(turn, &sandbox, max_steps, max_recovery, run_index, tx).await {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => last_err = Some(e),
        }
    }
    if outcomes.is_empty() {
        if let Some(e) = last_err {
            return Err(e);
        }
    }
    let report = AgenticReport::from_outcomes(&outcomes);
    Ok(if truncated { report.with_truncation(k) } else { report })
}

/// Run ONE agentic attempt: the stateful `while step < max_steps` loop (iterative,
/// no async recursion). Each turn it sends the running transcript to the model,
/// parses a JSON tool call, looks it up in the sandbox, and injects the
/// deterministic result back as text. Fires a `TrajectoryStep` per turn into `tx`.
/// Terminates on the EndStateRule (success), a model yield (hallucinated/malformed
/// failure), or the step cap (infinite loop).
pub async fn run_once<M: ModelTurn>(
    turn: &M,
    sandbox: &DeterministicSandbox,
    max_steps: u32,
    max_recovery: u8,
    run_index: u32,
    tx: &UnboundedSender<TrajectoryStep>,
) -> AppResult<RunOutcome> {
    // A slow turn (reasoning model, or a model Ollama spilled onto the CPU) gets a much larger
    // per-step cap so a genuinely-progressing generation isn't killed as a false `TurnTimeout`
    // — the same "don't score a hardware limit as a capability failure" principle as the
    // num_predict fix. A terse, fully-resident model keeps the tight default.
    let step_timeout = if turn.slow_inference() { STEP_TIMEOUT.saturating_mul(SLOW_STEP_MULTIPLIER) } else { STEP_TIMEOUT };
    run_once_inner(turn, sandbox, max_steps, max_recovery, step_timeout, run_index, tx).await
}

/// Per-step wall-clock budget. The streaming HTTP client has no body deadline, so a
/// stalled model would otherwise hang the loop forever; a turn over this budget ends
/// the run as `TurnTimeout`. Generous vs. local tok/s so a legitimately slow turn
/// (long generation, a fault-injected retry) isn't killed.
const STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// How much the per-step cap is multiplied for a `slow_inference` turn (a reasoning model, or
/// a model spilled onto the CPU). CPU-offloaded generation runs several times slower than
/// resident GPU inference, and a `<think>` scratchpad is a long generation on top; 180s×4 = 12
/// minutes/turn keeps a legitimately-slow-but-progressing turn alive while `PER_RUN_BUDGET`
/// (whole-run) and the loop's own stall guards still bound a truly wedged model.
const SLOW_STEP_MULTIPLIER: u32 = 4;

/// `run_once` with an injectable per-step timeout (so the timeout path is testable
/// without waiting the full budget).
#[allow(clippy::too_many_arguments)]
async fn run_once_inner<M: ModelTurn>(
    turn: &M,
    sandbox: &DeterministicSandbox,
    max_steps: u32,
    max_recovery: u8,
    step_timeout: std::time::Duration,
    run_index: u32,
    tx: &UnboundedSender<TrajectoryStep>,
) -> AppResult<RunOutcome> {
    // Track the tool-call dialect across the run and stamp it onto the outcome ONCE here,
    // so the many terminal returns inside `run_steps` stay untouched (builder seam).
    let mut dialect = ToolCallDialect::Standard;
    let outcome =
        run_steps(turn, sandbox, max_steps, max_recovery, step_timeout, run_index, tx, &mut dialect).await?;
    Ok(outcome.with_dialect(dialect))
}

/// The stateful step loop. `dialect` is an out-param the extract site updates the first
/// time a turn is recovered from a non-standard grammar; `run_once_inner` stamps it on the
/// returned outcome.
#[allow(clippy::too_many_arguments)]
async fn run_steps<M: ModelTurn>(
    turn: &M,
    sandbox: &DeterministicSandbox,
    max_steps: u32,
    max_recovery: u8,
    step_timeout: std::time::Duration,
    run_index: u32,
    tx: &UnboundedSender<TrajectoryStep>,
    dialect: &mut ToolCallDialect,
) -> AppResult<RunOutcome> {
    // Act-tasks must route every result — including the final report — through a tool;
    // abstain-tasks keep the plain-text option (prose IS the correct output there). Gating
    // here is the G1 fix for the prompt↔grader contradiction (a correct prose answer to a
    // RequireAll task otherwise yields → HallucinatedCompletion).
    let terminal = match &sandbox.end_state {
        EndStateRule::ExpectAbstainingText => TerminalGuidance::PlainTextOk,
        EndStateRule::RequireAll(_) | EndStateRule::RequireSequence(_) | EndStateRule::RequireEndState(_) => {
            TerminalGuidance::MustUseTools
        }
    };
    let system = build_system_for(&sandbox.tools, terminal);
    let mut convo = Conversation::new(sandbox.initial_prompt.clone());
    let mut output_tokens = 0u32;
    let mut next_cp = 0usize; // progress through a RequireSequence end-state
    // RequireAll (v2): per-checkpoint consumed flags (unordered, consume-once).
    let mut satisfied: Vec<bool> = match &sandbox.end_state {
        EndStateRule::RequireAll(cps) => vec![false; cps.len()],
        _ => Vec::new(),
    };
    let mut state = SandboxState::new(); // per-run fault attempt counters (Driver B)
    // Per-run MUTABLE web-UI state (Slice 3) — fresh each run, NEVER in the shared sandbox (which
    // holds only the immutable spec). Mirrors the per-run `SandboxState` lifecycle.
    let mut web_ui = match &sandbox.responder {
        ResponderKind::WebUi(spec) => Some(WebUiState::from_spec(spec)),
        _ => None,
    };
    let mut recoveries = 0u8; // schema corrections used this run (Driver D)
    let mut hit_schema_error = false; // this run emitted a schema-invalid call
    let mut schema_recovered = false; // ...and later produced a valid one
    let mut unknown_tools = 0u32; // decoy / unknown-tool calls this run (Phase 9 distraction signal)
    let mut prev_turn_sig: Option<Vec<String>> = None; // canonical calls of the previous turn
    let mut stalled_repeats = 0u32; // consecutive identical, no-progress turns (loop detector)
    let mut no_progress_streak = 0u32; // consecutive no-checkpoint turns, sig-agnostic (busy-loop guard)

    // Sized once per run from the step cap: the transcript only grows within this run,
    // so a single window covers every step. Keeps the prefix-KV cache from being busted
    // by an overflow-driven context-shift (see `agentic_num_ctx`).
    let num_ctx = agentic_num_ctx(max_steps);
    for step_index in 0..max_steps {
        // The per-turn output cap. A reasoning model gets a tier-scaled budget so its
        // `<think>` scratchpad doesn't truncate the call; both thinking and non-thinking now
        // clear a batched multi-call turn (see `difficulty::passk::max_tokens_for`).
        let base_predict = turn.max_output_tokens();

        // Run the turn, retrying ONLY when it was cut off at the `num_predict` cap
        // (`finish_reason == "length"`) AND parsed to zero calls. The retry re-runs the SAME
        // step against the SAME transcript with a larger, context-clamped budget: the truncated
        // partial is discarded (never appended — `convo` is untouched, so `convo.render()` is
        // identical across attempts), and this loop does NOT advance `step_index`, so a re-roll
        // never costs the model a turn of its horizon. Eval runs at temperature 0, so a larger
        // budget on the same prompt deterministically resumes the same generation rather than
        // rolling a different one. Only the KEPT attempt's tokens count toward effort (a
        // discarded re-roll must not inflate `output_tokens`).
        let mut attempt_predict = base_predict;
        let mut truncation_retries = 0u32;
        let (raw, stats) = loop {
            // Built inline (not a closure) so no borrow of `convo` outlives this loop — the
            // step below needs `&mut convo`. `convo` is untouched across retries, so
            // `convo.render()` is identical each attempt (the "discard partial" guarantee).
            let spec = GenerateSpec {
                model: String::new(),
                prompt: convo.render(),
                system: Some(system.clone()),
                options: Some(GenerateOptions {
                    temperature: Some(0.0),
                    // Harness default: stop greedy repetition collapse. Header-supplied
                    // value still wins (see `merge_eval_options`).
                    repeat_penalty: Some(EVAL_REPEAT_PENALTY),
                    num_predict: Some(attempt_predict),
                    num_ctx: Some(num_ctx),
                    ..Default::default()
                }),
                keep_alive: None,
            };
            let (raw, stats) = match tokio::time::timeout(step_timeout, turn.run(&spec)).await {
                Ok(r) => r?, // backend returned; an Err propagates (infra fault → run skipped upstream)
                Err(_elapsed) => {
                    // The turn blew the wall-clock — a stalled model. Terminal: a hanging
                    // agent isn't production-ready, so it counts as a failure (not a skip).
                    let _ = tx.send(TrajectoryStep {
                        run_index,
                        step_index,
                        raw_output: String::new(),
                        injection: None,
                        kind: StepKind::TurnTimeout,
                        env: EnvView::None,
                        cache_n: None, // no model response on a timeout
                        prefill_tokens: None,
                        prefill_ms: None,
                    });
                    return Ok(RunOutcome::failure(step_index + 1, output_tokens, FailureKind::TurnTimeout)
                        .with_schema(hit_schema_error, schema_recovered)
                        .with_unknown_tools(unknown_tools));
                }
            };
            let truncated = stats.finish_reason.as_deref() == Some("length");
            let zero_calls = {
                let clean = if turn.is_thinking() { strip_think(&raw) } else { raw.clone() };
                extract_calls_dialect(&clean).is_none()
            };
            if truncated && zero_calls && truncation_retries < TRUNCATION_RETRY_LIMIT {
                // Context-safety guard: clamp the retry to the generation room ACTUALLY left this
                // turn. True prompt occupancy is `cache_n + prompt_eval_count` (llama.cpp serves
                // most of a reused prefix from cache and reports only the recomputed part as
                // `prompt_eval_count`, so ignoring `cache_n` would wildly overstate headroom). If
                // occupancy is unknown (missing/0) OR the transcript leaves no room for even the
                // base budget, retrying would over-grant and overflow `num_ctx` — so we DON'T
                // retry; the no-call ladder below labels it honestly as `Truncated`.
                let occupancy = stats
                    .prompt_eval_count
                    .map(|p| p.saturating_add(stats.cache_n.unwrap_or(0)))
                    .filter(|&p| p > 0);
                if let Some(prefill) = occupancy {
                    let headroom = num_ctx.saturating_sub(prefill);
                    if headroom > base_predict {
                        attempt_predict = base_predict.saturating_mul(2).min(headroom);
                        truncation_retries += 1;
                        continue; // re-run this same step; transcript untouched
                    }
                }
            }
            break (raw, stats);
        };
        output_tokens += stats.eval_count.unwrap_or(0);
        // Per-turn prompt-cache reuse (llama.cpp `timings.cache_n`); None for other
        // backends. Captured here so each streamed step carries its own turn's value.
        let cache_n = stats.cache_n;
        let prefill_tokens = stats.prompt_eval_count; // prompt_n = recomputed; total = cache_n + this
        let prefill_ms = stats.prompt_eval_ms;
        let send = |kind: StepKind, injection: Option<String>, env: EnvView| {
            let _ = tx.send(TrajectoryStep { run_index, step_index, raw_output: raw.clone(), injection, kind, env, cache_n, prefill_tokens, prefill_ms });
        };

        // For a reasoning model, parse and persist the `<think>`-stripped output: its inner
        // braces must not be mis-parsed as a tool call, and re-sending the scratchpad every
        // step would bloat the prefix-KV cache. The streamed `raw_output` above keeps the
        // FULL text so the UI can still show the reasoning. A terse model is unchanged
        // (`clean == raw`), so the non-thinking path stays byte-for-byte identical.
        let clean = if turn.is_thinking() { strip_think(&raw) } else { raw.clone() };

        // The model emits ZERO or MORE tool calls per turn (the system prompt invites a
        // JSON array). We process EVERY parsed call in array order — dropping all but the
        // first silently half-executes a correct batched agent. `extract_calls` is lenient:
        // it returns the parseable calls and ignores unparseable slices, so `malformed_json`
        // stays a whole-output property (zero calls parsed → the no-call arm below).
        let calls = match extract_calls_dialect(&clean) {
            None => match &sandbox.end_state {
                // Declined to call any tool, exactly as the task demanded.
                EndStateRule::ExpectAbstainingText => {
                    send(StepKind::EndStateReached, None, EnvView::None);
                    return Ok(RunOutcome::success(step_index + 1, output_tokens));
                }
                // Yielded (no call) without completing the required checkpoints / reaching the
                // target UI state.
                EndStateRule::RequireSequence(_) | EndStateRule::RequireAll(_) | EndStateRule::RequireEndState(_) => {
                    let (kind, failure) = if stats.finish_reason.as_deref() == Some("length") {
                        // The turn was cut off at the output cap and (after the context-clamped
                        // retry above, if any) still parsed to zero calls — a harness/hardware
                        // truncation, NOT a capability failure. Checked FIRST so a length-cut
                        // turn is never laundered into Malformed / Hallucinated / EmptyOutput.
                        (StepKind::Truncated, FailureKind::Truncated)
                    } else if is_empty_output(&clean) {
                        // The model produced nothing usable (empty / whitespace / a lone
                        // punctuation char before its stop token). A generation/template
                        // artifact, NOT a claimed-but-false completion. Checked first: an
                        // empty string trivially matches none of the others, and labeling it
                        // `Hallucinated` reads as "the model lied about finishing".
                        (StepKind::EmptyOutput, FailureKind::EmptyOutput)
                    } else if looks_like_foreign_dialect(&clean) {
                        // The model spoke a non-JSON tool dialect the parser (and a real
                        // deployment) can't read — a template/dialect artifact, NOT a
                        // hallucination or broken JSON. Checked before the
                        // braces-but-no-object `Malformed` heuristic.
                        (StepKind::ForeignDialect, FailureKind::ForeignDialect)
                    } else if looks_like_broken_json(&clean) {
                        (StepKind::MalformedJson, FailureKind::Malformed)
                    } else if reported_in_prose(&sandbox.end_state, &satisfied, next_cp, &clean) {
                        // G3: did ALL the work, only failed to route the final answer through
                        // the reporter tool — content-correct, wrong-channel. NOT a hallucination.
                        (StepKind::ReportedInProse, FailureKind::ReportedInProse)
                    } else {
                        (StepKind::HallucinatedCompletion, FailureKind::Hallucinated)
                    };
                    send(kind, None, EnvView::None);
                    return Ok(RunOutcome::failure(step_index + 1, output_tokens, failure)
                        .with_schema(hit_schema_error, schema_recovered)
                        .with_unknown_tools(unknown_tools));
                }
            },
            Some((calls, d)) => {
                // A non-standard grammar (e.g. Harmony) sticks for the run — surfaced later.
                if d != ToolCallDialect::Standard {
                    *dialect = d;
                }
                calls
            }
        };

        // The per-turn environment snapshot for the visual replay, derived from the turn's
        // calls (the env picks its representative action — e.g. the last file read, even when
        // batched before a reply). A pure fn of the immutable responder + calls, so the picture
        // can never disagree with the score. `None` for non-env tasks.
        let turn_env = env_view(&sandbox.responder, &calls, web_ui.as_ref());

        // Acted (called ≥1 tool) when the task wanted a plain-text abstention — declining
        // was correct, so this is a failure.
        if matches!(sandbox.end_state, EndStateRule::ExpectAbstainingText) {
            send(StepKind::HallucinatedCompletion, None, turn_env.clone());
            return Ok(RunOutcome::failure(step_index + 1, output_tokens, FailureKind::Hallucinated));
        }

        // Snapshot end-state progress before processing this turn's calls, so the loop
        // detector below can tell a productive turn (advanced a checkpoint) from a stalled
        // one. Sequence uses `next_cp`; RequireAll counts satisfied checkpoints.
        let progress_before = next_cp + satisfied.iter().filter(|&&s| s).count();

        // Step 1 — FORBIDDEN PRE-SCAN (the trap dominates the whole turn). A forbidden
        // action emitted ANYWHERE in the array springs the trap, even alongside a call that
        // would complete the end-state — the model must not launder a trap by batching it
        // with the winning move. Restricted to SCHEMA-VALID calls so a malformed forbidden
        // call still takes the recovery path below (can't trap via malformed) — preserving
        // the prior schema-before-forbidden ordering.
        for call in &calls {
            let schema_ok = sandbox.tools.is_empty() || endstate::validate_call(call, &sandbox.tools).is_ok();
            if schema_ok && sandbox.must_not_call.iter().any(|m| m.matches(call)) {
                send(StepKind::ForbiddenCall, None, turn_env.clone());
                return Ok(RunOutcome::failure(step_index + 1, output_tokens, FailureKind::ForbiddenCall)
                    .with_schema(hit_schema_error, schema_recovered)
                    .with_unknown_tools(unknown_tools));
            }
        }

        // Step 2 — process each call in array order. `model_pushed` defers pushing the raw
        // model turn until the FIRST injected result, so a turn whose first call completes
        // the end-state (or terminates) pushes nothing to the transcript — byte-identical to
        // the old single-call terminal path. `turn_lines` collects each call's injected line
        // so the turn streams ONE `TrajectoryStep` (the UI renders one card per turn) with
        // every result joined, not just the first.
        let mut model_pushed = false;
        let mut turn_lines: Vec<(StepKind, String)> = Vec::new();
        for call in &calls {
            // 3a — Driver D semantic validation (only when the task declares schemas). An
            // invalid call injects a correction and burns ONE recovery, then CONTINUES to the
            // next call (no-drop: a sibling's schema error never discards a valid call).
            // Exhausting the budget is terminal (MalformedSchema).
            if !sandbox.tools.is_empty() {
                if let Err(msg) = endstate::validate_call(call, &sandbox.tools) {
                    hit_schema_error = true;
                    if recoveries >= max_recovery {
                        send(StepKind::SchemaError, join_injection(&turn_lines), turn_env.clone()); // terminal: budget spent
                        return Ok(RunOutcome::failure(step_index + 1, output_tokens, FailureKind::MalformedSchema)
                            .with_schema(true, schema_recovered)
                            .with_unknown_tools(unknown_tools));
                    }
                    recoveries += 1;
                    let err = format!("[Schema error: {msg}]");
                    ensure_model_pushed(&mut convo, &clean, &mut model_pushed);
                    convo.push_tool_result(&err);
                    turn_lines.push((StepKind::SchemaError, tool_result_line(&err)));
                    continue;
                }
                if hit_schema_error && !schema_recovered {
                    schema_recovered = true; // a valid call after a schema error is the recovery
                }
            }
            // 3b — Driver B fault trap, BEFORE any checkpoint advance, so a trapped call can
            // never be a fake pass. The counter is per-call; a robust agent retries/reports.
            if let Some(err) = state.fault_for(call, &sandbox.faults) {
                ensure_model_pushed(&mut convo, &clean, &mut model_pushed);
                convo.push_tool_result(&err);
                turn_lines.push((StepKind::ToolError, tool_result_line(&err)));
                continue;
            }
            // 3c — apply the STATEFUL web-UI action NOW (the mutation IS the env's effect, and the
            // completion check below reads the post-action state). For a stateless env nothing is
            // applied here — `respond` is deferred to 3f so a terminal checkpoint call is never
            // called (and so never mis-counted as an unknown tool).
            let applied = match web_ui.as_mut() {
                Some(st) => Some(match st.apply(call, &sandbox.recognized_tools) {
                    Some(r) => (StepKind::ToolCall, r),
                    None => {
                        unknown_tools += 1;
                        (StepKind::UnknownTool, UNKNOWN_TOOL.to_string())
                    }
                }),
                None => None,
            };
            // 3d — end-state progress: ordered (RequireSequence) / unordered consume-once
            // (RequireAll, wildcard-aware) / exact final-state match (RequireEndState, read AFTER
            // the apply above). Two calls in one turn satisfy two distinct checkpoints.
            let complete = match &sandbox.end_state {
                EndStateRule::RequireSequence(cps) => {
                    if endstate::checkpoint_matches(&cps[next_cp], call) {
                        next_cp += 1;
                    }
                    next_cp == cps.len()
                }
                EndStateRule::RequireAll(cps) => {
                    for (i, cp) in cps.iter().enumerate() {
                        if !satisfied[i] && endstate::checkpoint_matches_v2(cp, call) {
                            satisfied[i] = true;
                            break; // a call consumes at most one checkpoint
                        }
                    }
                    satisfied.iter().all(|&s| s)
                }
                EndStateRule::RequireEndState(target) => web_ui.as_ref().is_some_and(|st| st.matches(target)),
                EndStateRule::ExpectAbstainingText => unreachable!("handled above"),
            };
            // The per-call replay snapshot: for the stateful web-UI it must reflect the
            // POST-action state, so rebuild it from the mutated `web_ui`; otherwise the
            // pre-computed `turn_env`.
            let call_env = match web_ui.as_ref() {
                Some(st) => EnvView::WebUi(st.view(&calls)),
                None => turn_env.clone(),
            };
            // 3e — terminal success the instant the end-state is reached (race-free: the forbidden
            // pre-scan already cleared the turn, so a `must_not_call` can't be laundered here).
            if complete {
                send(StepKind::EndStateReached, join_injection(&turn_lines), call_env);
                return Ok(RunOutcome::success(step_index + 1, output_tokens)
                    .with_schema(hit_schema_error, schema_recovered)
                    .with_unknown_tools(unknown_tools));
            }
            // 3f — not complete: inject this call's tool result and continue. The web-UI result
            // was already produced by the apply in 3c; a stateless env reads `respond` now (so a
            // terminal checkpoint call above never reached `respond` → never mis-counted unknown).
            let (kind, result) = match applied {
                Some(kr) => kr,
                None => match sandbox.respond(call) {
                    Some(r) => (StepKind::ToolCall, r),
                    None => {
                        unknown_tools += 1; // a decoy or hallucinated tool — no mock exists
                        (StepKind::UnknownTool, UNKNOWN_TOOL.to_string())
                    }
                },
            };
            ensure_model_pushed(&mut convo, &clean, &mut model_pushed);
            convo.push_tool_result(&result);
            turn_lines.push((kind, tool_result_line(&result)));
        }

        // Turn complete, NON-terminal: stream one step carrying every injected result + the
        // environment snapshot for the visual replay. For the stateful web-UI, rebuild the view
        // from the POST-action state so the replay shows the resulting UI.
        let (kind, injection) = summarize_turn(&turn_lines);
        let final_env = match web_ui.as_ref() {
            Some(st) => EnvView::WebUi(st.view(&calls)),
            None => turn_env,
        };
        send(kind, injection, final_env);

        // Loop detector, two tiers. (a) A turn that re-emits the EXACT same calls as the
        // previous turn AND advanced no checkpoint is a hard stall — fail fast after
        // `STALL_REPEAT_LIMIT`. (b) A turn that advances NO checkpoint for
        // `STALL_NO_PROGRESS_LIMIT` turns in a row, EVEN IF its calls vary slightly, is a
        // busy loop (the 37-step "keep re-searching" trajectory that dodges the exact-match
        // check) — also `InfiniteLoop`. Either resets on real checkpoint progress.
        let progressed = next_cp + satisfied.iter().filter(|&&s| s).count() > progress_before;
        let sig: Vec<String> = calls.iter().map(canonical).collect();
        if !progressed && prev_turn_sig.as_ref() == Some(&sig) {
            stalled_repeats += 1;
        } else {
            stalled_repeats = 0;
        }
        prev_turn_sig = Some(sig);
        no_progress_streak = if progressed { 0 } else { no_progress_streak + 1 };
        if stalled_repeats + 1 >= STALL_REPEAT_LIMIT || no_progress_streak >= STALL_NO_PROGRESS_LIMIT {
            let _ = tx.send(TrajectoryStep {
                run_index,
                step_index: step_index + 1,
                raw_output: String::new(),
                injection: None,
                kind: StepKind::InfiniteLoop,
                env: EnvView::None,
                cache_n: None, // synthetic terminal step, no model response
                prefill_tokens: None,
                prefill_ms: None,
            });
            return Ok(RunOutcome::failure(step_index + 1, output_tokens, FailureKind::InfiniteLoop)
                .with_schema(hit_schema_error, schema_recovered)
                .with_unknown_tools(unknown_tools));
        }
    }

    let _ = tx.send(TrajectoryStep {
        run_index,
        step_index: max_steps,
        raw_output: String::new(),
        injection: None,
        kind: StepKind::InfiniteLoop,
        env: EnvView::None,
        cache_n: None, // synthetic terminal step, no model response
        prefill_tokens: None,
        prefill_ms: None,
    });
    Ok(RunOutcome::failure(max_steps, output_tokens, FailureKind::InfiniteLoop)
        .with_schema(hit_schema_error, schema_recovered)
        .with_unknown_tools(unknown_tools))
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;

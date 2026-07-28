//! `qm cliff` — the Context Stress Test, headless: ramp prompt depth toward
//! `--max-tokens` and classify where tool-call accuracy collapses. Wraps the same
//! Tauri-free engine the GUI's Audit tab drives (`inference::eval::cliff`), prompt
//! path, greedy (temp 0) so the probe is reproducible run-to-run.

use crate::cli::doctor::probe::probe_backend;
use crate::cli::run::{openai_reasons, RunMode, RunOptions};
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::{endpoint, remote_config};
use crate::inference::eval::agentic::difficulty::passk::{answer_tokens_for, ThinkPreset};
use crate::inference::eval::agentic::model_turn::BackendTurn;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::cliff::{build_ladder, run_cliff_with, CliffBudget, CliffReport, CliffSource, DEFAULT_DEPTHS};
use crate::inference::eval::readiness::types::CliffStatus;
use crate::inference::ollama::ollama_show::probe_supports_thinking;
use tokio_util::sync::CancellationToken;

pub struct CliffOptions {
    pub run: RunOptions,
    /// Ceiling for the padding ladder (the deepest rung's target tokens).
    pub max_tokens: u32,
    /// Number of ladder rungs (min 2: baseline + deepest).
    pub steps: u32,
    pub source: CliffSource,
    /// Native tool-calling path (vs the default prompt-based proxy).
    pub native: bool,
    /// Sampling params. `None` → greedy temp-0 (reproducible). When set,
    /// temperature/top_p/… sample; `num_ctx` is still forced to the ladder window.
    pub params: Option<crate::persistence::prompts::schema::InferenceParams>,
}

/// The answer-delivery mandate per task — MustUseTools for a stateful/ordered
/// end-state, else PlainTextOk. Mirrors `readiness_cmd::cliff_terminal` (kept local
/// to avoid coupling the CLI to the Tauri command layer).
fn cliff_terminal(
    task: &crate::inference::eval::toolcall::tasks::ToolTask,
) -> crate::inference::eval::toolcall::prompt::TerminalGuidance {
    use crate::inference::eval::agentic::sandbox::EndStateRule;
    use crate::inference::eval::toolcall::prompt::TerminalGuidance;
    match task.agentic.as_ref().map(|s| &s.end_state) {
        Some(EndStateRule::RequireAll(_)) | Some(EndStateRule::RequireSequence(_)) => TerminalGuidance::MustUseTools,
        _ => TerminalGuidance::PlainTextOk,
    }
}

pub enum CliffOutcome {
    Unreachable { backend: BackendKind, endpoint: String },
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    UnknownCollection { id: String },
    BadCollectionFile { path: String, reason: String },
    /// `--thinking standard|deep` where reasoning won't actually happen — refused
    /// loudly (mirrors `RunOutcome::ThinkingUnsupported`) instead of probing a ladder
    /// whose scratchpad silently no-ops.
    ThinkingUnsupported { backend: BackendKind, model: String },
    Probed(CliffReport),
}

/// Exit code for a cliff verdict — the documented contract: no-cliff 0 ·
/// collapsed 10 · inconclusive 11 · broken-baseline 20.
pub fn cliff_exit(status: &CliffStatus) -> i32 {
    match status {
        CliffStatus::NoCliff { .. } => 0,
        CliffStatus::Collapsed { .. } => 10,
        CliffStatus::Inconclusive { .. } => 11,
        CliffStatus::Broken { .. } | CliffStatus::NotProbed => 20,
    }
}

/// Human render: one line per rung + the classified status (mirrors the Audit chart).
pub fn render_cliff(r: &CliffReport) -> String {
    let mut out = String::new();
    for (i, p) in r.points.iter().enumerate() {
        let tally = match (p.passed, p.trials) {
            (Some(pass), Some(tr)) => format!("  ({pass}/{tr})"),
            _ => String::new(), // sample size only when actually measured
        };
        let acc = p
            .composite
            .map(|c| format!("accuracy {:>5.1}%{tally}", c * 100.0))
            .unwrap_or_else(|| "unmeasured".into());
        out.push_str(&format!("rung {}: ~{:>6} tok · {}\n", i + 1, p.verified_tokens, acc));
    }
    out.push_str(&match &r.status {
        CliffStatus::NoCliff { tested } => format!("STATUS: ✓ no cliff — accuracy maintained up to ≈{tested} tokens\n"),
        CliffStatus::Collapsed { depth } => format!("STATUS: ✗ collapsed at ≈{depth} tokens\n"),
        CliffStatus::Broken { tested } => format!("STATUS: ✗ broken baseline — failing at the smallest context (tested to ≈{tested})\n"),
        CliffStatus::Inconclusive { trials } => {
            format!("STATUS: ? inconclusive — {trials} trials/rung can't resolve a cliff from noise; add tasks or repeats\n")
        }
        CliffStatus::NotProbed => "STATUS: not probed\n".into(),
    });
    out
}

/// Run the probe. Preflight (reachability + model presence via the doctor probe),
/// greedy decoding, context window sized to the deepest rung + headroom. Progress
/// (one line per rung) goes to stderr via `on_rung`.
pub async fn run_cliff_probe(opts: CliffOptions) -> AppResult<CliffOutcome> {
    let tasks = match super::run::load_collection(&opts.run.collection) {
        Ok(t) => t.tasks,
        Err(super::run::CollectionError::UnknownBuiltin) => {
            return Ok(CliffOutcome::UnknownCollection { id: opts.run.collection })
        }
        Err(super::run::CollectionError::BadFile(reason)) => {
            return Ok(CliffOutcome::BadCollectionFile { path: opts.run.collection, reason })
        }
    };

    let probed = probe_backend(opts.run.backend, opts.run.base.as_deref(), Some(&opts.run.model), opts.run.api_key.as_deref()).await;
    if !probed.reachable {
        return Ok(CliffOutcome::Unreachable { backend: opts.run.backend, endpoint: probed.endpoint });
    }
    if !probed.models.iter().any(|m| m == &opts.run.model) {
        return Ok(CliffOutcome::ModelNotFound { backend: opts.run.backend, model: opts.run.model, available: probed.models });
    }

    let ep = opts
        .run
        .base
        .clone()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| endpoint::base_url(opts.run.backend));
    match opts.run.backend {
        BackendKind::VLlm => remote_config::set_vllm(Some(ep.clone()), opts.run.api_key.clone()),
        BackendKind::SgLang => remote_config::set_sglang(Some(ep.clone()), opts.run.api_key.clone()),
        _ => {}
    }

    // `--thinking`: Lean = reasoning off (the pre-preset budget); Standard/Deep add a
    // scratchpad banded to each rung's depth (same semantics as `qm run`). Guard first:
    // a preset that can't actually reason must refuse loudly, not silently no-op.
    let is_thinking = !matches!(opts.run.think, ThinkPreset::Lean);
    if is_thinking {
        let reasons = match opts.run.backend {
            BackendKind::Ollama => probe_supports_thinking(&ep, &opts.run.model).await,
            _ => openai_reasons(&ep, &opts.run.model, opts.run.api_key.as_deref()).await,
        };
        if !reasons {
            return Ok(CliffOutcome::ThinkingUnsupported { backend: opts.run.backend, model: opts.run.model });
        }
    }
    let budget = CliffBudget { is_thinking, preset: opts.run.think };

    // A window that fits the deepest rung — plus, for a thinking run, the deepest
    // rung's scratchpad. Greedy (temp 0) by default so the probe reproduces
    // run-to-run; user params sample instead. `num_ctx` is ALWAYS forced to the
    // ladder window — a smaller user value would truncate the deepest rung.
    let needed_ctx = opts.max_tokens.saturating_add(budget.headroom(opts.max_tokens));
    let mut options = opts
        .params
        .as_ref()
        .map(crate::commands::prompt::prompt_options::to_generate_options)
        .unwrap_or_default();
    if options.temperature.is_none() {
        options.temperature = Some(0.0);
    }
    options.num_ctx = Some(needed_ctx);

    let cancel = CancellationToken::new();
    let ladder = build_ladder(opts.max_tokens, opts.steps);
    let mut on_rung = |done: usize, total: usize, point: &crate::inference::eval::cliff::CliffPoint| {
        let acc = point.composite.map(|c| format!("{:.0}%", c * 100.0)).unwrap_or_else(|| "—".into());
        eprintln!("· rung {done}/{total}: ~{} tok · {acc}", point.verified_tokens);
    };
    let mut no_step = |_s: crate::inference::eval::cliff::StepProgress| {};

    let report = if opts.native {
        // Native tool-calling cliff — a fresh NativeToolTurn per task carrying its
        // schemas (mirrors the GUI native path, readiness_cmd.rs make_native).
        let make_native = |task: &crate::inference::eval::toolcall::tasks::ToolTask| {
            crate::inference::eval::agentic::model_turn::NativeToolTurn {
                backend: opts.run.backend,
                endpoint: ep.clone(),
                model: opts.run.model.clone(),
                tools: task.tools.clone(),
                options: Some(options.clone()),
                terminal: cliff_terminal(task),
                // Fallback only — the engine pins each rung's depth-banded budget on
                // the spec, which wins the merge (see `merge_eval_options`).
                max_tokens: answer_tokens_for(Tier::Easy),
                is_thinking,
            }
        };
        crate::inference::eval::cliff::run_cliff_with_factory(
            &make_native, &opts.run.model, &tasks, &opts.source, &ladder, &DEFAULT_DEPTHS, needed_ctx, budget, &cancel, &mut on_rung, &mut no_step,
        )
        .await?
    } else {
        let turn = BackendTurn {
            backend: opts.run.backend,
            endpoint: ep,
            model: opts.run.model.clone(),
            cancel: cancel.clone(),
            options: Some(options),
            keep_alive: None,
            is_thinking, // thinking runs reason before the call; Lean mirrors the old liveness probe
            max_tokens: answer_tokens_for(Tier::Easy),
            cpu_offloaded: false,
            ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
            stop_cache: Default::default(),
        };
        run_cliff_with(&turn, &opts.run.model, &tasks, &opts.source, &ladder, &DEFAULT_DEPTHS, needed_ctx, budget, &cancel, &mut on_rung, &mut no_step).await?
    };
    // `RunMode` is unused here (prompt-only probe) but kept on RunOptions for parity.
    let _ = RunMode::PromptBased;
    Ok(CliffOutcome::Probed(report))
}

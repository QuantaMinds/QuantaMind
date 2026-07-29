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
    /// Flat per-turn output cap for EVERY rung (experimental control) — overrides
    /// the depth-banded thinking budget, so depth is the only variable that moves.
    pub cap: Option<u32>,
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
    /// llama.cpp pins its context at launch; the running server's window can't hold the
    /// requested ladder. Refused up front with both levers (mirrors the GUI gate) —
    /// previously this died mid-ladder on an opaque "prompt is larger than the context
    /// window" rejection, or silently dropped the deepest rungs.
    WindowTooSmall { running_ctx: u32, needed_ctx: u32, usable_max_tokens: u32 },
    /// `--mode native` on a backend/model that can't run native tool-calling (MLX has no
    /// tool API; an Ollama model whose template lacks `.Tools` 400s on every call).
    /// Refused up front (mirrors the GUI gate in `run_context_cliff`) — previously this
    /// died mid-ladder on an opaque `[QM-INTERNAL] … does not support tools` error.
    NativeUnsupported { backend: BackendKind, model: String },
    Probed(CliffReport),
}

/// Exit code for a cliff verdict — the documented contract: no-cliff 0 ·
/// collapsed 10 · inconclusive 11 · broken-baseline 20.
pub fn cliff_exit(status: &CliffStatus) -> i32 {
    match status {
        CliffStatus::NoCliff { .. } => 0,
        CliffStatus::Collapsed { .. } => 10,
        CliffStatus::Inconclusive { .. } => 11,
        // Budget-bound measurement — a config outcome, distinct from every model verdict.
        CliffStatus::BudgetLimited { .. } => 12,
        // Baseline grazed the cap — refused before any padded rung; also a config outcome.
        CliffStatus::CapMarginal { .. } => 13,
        CliffStatus::Broken { .. } | CliffStatus::NotProbed => 20,
    }
}

/// Human render: one line per rung + the classified status (mirrors the Audit chart).
/// A rung with failures names WHICH tasks failed (`by_task`), so a verdict driven by a
/// single task is visible in the terminal, not only in the JSON.
pub fn render_cliff(r: &CliffReport) -> String {
    let mut out = String::new();
    for (i, p) in r.points.iter().enumerate() {
        // Three-bucket rule: a cap-affected rung prints passed / failed / died-at-cap —
        // never a single rate (dropping cap cells overstates, folding them understates).
        let acc = match (p.passed, p.trials) {
            (Some(pass), Some(tr)) if p.cap_deaths > 0 => {
                let failed = tr - pass - p.cap_deaths;
                format!("{pass} passed · {failed} failed · {} died-at-cap  ({tr} cells over {} tasks)", p.cap_deaths, p.by_task.len())
            }
            (Some(pass), Some(tr)) => p
                .composite
                .map(|c| format!("accuracy {:>5.1}%  ({pass}/{tr} over {} tasks)", c * 100.0, p.by_task.len()))
                .unwrap_or_else(|| "unmeasured".into()),
            _ => p.composite.map(|c| format!("accuracy {:>5.1}%", c * 100.0)).unwrap_or_else(|| "unmeasured".into()),
        };
        out.push_str(&format!("rung {}: ~{:>6} tok · {}\n", i + 1, p.verified_tokens, acc));
        let failing: Vec<String> = p
            .by_task
            .iter()
            .filter(|t| t.passed < t.trials)
            .map(|t| {
                let cap = if t.failed_cap_hits > 0 { format!(" ({} died at cap)", t.failed_cap_hits) } else { String::new() };
                format!("{} {}/{}{}", t.task_id, t.passed, t.trials, cap)
            })
            .collect();
        if !failing.is_empty() {
            out.push_str(&format!("        failures: {}\n", failing.join(" · ")));
        }
        // Amber early warning: passing tasks whose tightest cell sat within the headroom
        // floor of the cap — likely to fail at the next rung (greedy-calibrated).
        let near: Vec<String> = p
            .by_task
            .iter()
            .filter(|t| t.passed == t.trials)
            .filter_map(|t| {
                t.min_pass_headroom_milli
                    .filter(|h| *h < crate::inference::eval::cliff::engine::AMBER_HEADROOM_MILLI)
                    .map(|h| format!("{} ({}\u{2030} headroom)", t.task_id, h))
            })
            .collect();
        if !near.is_empty() {
            out.push_str(&format!("        near-cap: {}\n", near.join(" · ")));
        }
    }
    out.push_str(&match &r.status {
        CliffStatus::NoCliff { tested } => {
            let cap_total: u32 = r.points.iter().map(|p| p.cap_deaths).sum();
            if cap_total > 0 {
                format!(
                    "STATUS: ✓ no cliff on content — maintained up to ≈{tested} tokens; {cap_total} cell(s) \
                     died at the output cap along the way (budget events, excluded from the model claim — \
                     raise the budget to measure them)\n"
                )
            } else {
                format!("STATUS: ✓ no cliff — accuracy maintained up to ≈{tested} tokens\n")
            }
        }
        CliffStatus::Collapsed { depth, concentration } => {
            let mut line = format!("STATUS: ✗ collapsed at ≈{depth} tokens");
            // The collapse rung's Wilson 95% interval + sample, so the claim carries its
            // own uncertainty (never a bare point estimate).
            if let Some(p) = r.points.iter().find(|p| p.verified_tokens == *depth) {
                if let (Some(pass), Some(tr)) = (p.passed, p.trials) {
                    if let Some(w) = crate::inference::eval::cliff::stats::wilson_interval(pass, tr) {
                        line.push_str(&format!(
                            "  ({pass}/{tr} over {} tasks; Wilson 95%: {:.0}–{:.0}%)",
                            p.by_task.len(),
                            w.lo * 100.0,
                            w.hi * 100.0
                        ));
                    }
                }
            }
            line.push('\n');
            if let Some(c) = concentration {
                let verdict_note = if c.holds_without {
                    "collapse driven by that task — depth-general collapse NOT established"
                } else {
                    "collapse persists without it"
                };
                line.push_str(&format!(
                    "        low confidence — {} of {} failures from one task ({}, p≈{:.3}); {verdict_note}\n",
                    c.task_failures,
                    c.total_failures,
                    c.task_id,
                    c.p_value_milli as f64 / 1000.0
                ));
            }
            line
        }
        CliffStatus::BudgetLimited { depth, cap } => format!(
            "STATUS: ⚠ budget-limited at ≈{depth} tokens — every failure died at the {cap}-token \
             output cap (finish=length). This is a budget-bound measurement, not an established \
             model collapse: raise the budget (--thinking, or a larger cap) and re-run — \
             recovery means the model was starved; the same failures mean it loops.\n"
        ),
        CliffStatus::CapMarginal { cap, used_milli } => format!(
            "STATUS: ⚠ cap-marginal baseline — the tightest passing cell used {used_milli}‰ of the \
             {cap}-token output cap (gate: ≥900‰). The baseline only passed by grazing the cap, so \
             padded rungs would measure the output budget, not the model — none were run. Raise the \
             budget (--thinking standard/deep, or --cap) and re-run.\n"
        ),
        CliffStatus::Broken { tested } => format!("STATUS: ✗ broken baseline — failing at the smallest context (tested to ≈{tested})\n"),
        CliffStatus::Inconclusive { trials } => {
            format!("STATUS: ? inconclusive — {trials} trials/rung can't resolve a cliff from noise; add tasks or repeats\n")
        }
        CliffStatus::NotProbed => "STATUS: not probed\n".into(),
    });
    if let Some(t) = r.temperature.filter(|t| *t > 0.0) {
        out.push_str(&format!("        sampled at temperature {t} (from your params) — not comparable with greedy runs\n"));
    }
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
    let budget = CliffBudget { is_thinking, preset: opts.run.think, flat_cap: opts.cap };

    // Native preflight: same gate as the GUI (`run_context_cliff`) — a model/backend that
    // can't run native tool-calling must refuse loudly up front, not 400 mid-ladder.
    if opts.native && !crate::inference::eval::batch::probe_native_tools(opts.run.backend, &ep, &opts.run.model).await {
        return Ok(CliffOutcome::NativeUnsupported { backend: opts.run.backend, model: opts.run.model });
    }

    // llama.cpp preflight: the server pins its window at launch — measure against the
    // RUNNING window, not the model's GGUF maximum. Without this the deepest rungs
    // either 400 mid-ladder (killing the whole probe) or get dropped as unmeasurable.
    if opts.run.backend == BackendKind::LlamaCpp {
        let needed = opts.max_tokens.saturating_add(budget.headroom(opts.max_tokens));
        if let Some((_path, running_ctx)) = crate::inference::llama::llama_props::probe_props(&ep, 1500).await {
            if running_ctx < needed {
                let usable = running_ctx.saturating_sub(budget.headroom(running_ctx));
                return Ok(CliffOutcome::WindowTooSmall { running_ctx, needed_ctx: needed, usable_max_tokens: usable });
            }
        }
    }

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
    let options_temp = options.temperature;
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
    let mut report = report;
    // Stamp the decoding config the run actually used — greedy 0.0 unless the user's
    // params set one (metric comparability: a sampled depth is labeled as sampled).
    report.temperature = options_temp;
    Ok(CliffOutcome::Probed(report))
}

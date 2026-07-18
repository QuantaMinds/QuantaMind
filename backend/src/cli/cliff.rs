//! `qm cliff` — the Context Stress Test, headless: ramp prompt depth toward
//! `--max-tokens` and classify where tool-call accuracy collapses. Wraps the same
//! Tauri-free engine the GUI's Audit tab drives (`inference::eval::cliff`), prompt
//! path, greedy (temp 0) so the probe is reproducible run-to-run.

use crate::cli::doctor::probe::probe_backend;
use crate::cli::run::{RunMode, RunOptions};
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::{endpoint, remote_config};
use crate::inference::eval::agentic::difficulty::passk::answer_tokens_for;
use crate::inference::eval::agentic::model_turn::BackendTurn;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::cliff::{build_ladder, run_cliff_with, CliffReport, CliffSource, DEFAULT_DEPTHS};
use crate::inference::eval::readiness::types::CliffStatus;
use crate::inference::generate::generate_options::GenerateOptions;
use tokio_util::sync::CancellationToken;

/// Same headroom the GUI probe reserves above the deepest rung for system prompt,
/// needle, and the answer (mirrors `readiness_cmd::CLIFF_CTX_HEADROOM`).
const CTX_HEADROOM: u32 = 2048;

pub struct CliffOptions {
    pub run: RunOptions,
    /// Ceiling for the padding ladder (the deepest rung's target tokens).
    pub max_tokens: u32,
    /// Number of ladder rungs (min 2: baseline + deepest).
    pub steps: u32,
    pub source: CliffSource,
}

pub enum CliffOutcome {
    Unreachable { backend: BackendKind, endpoint: String },
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    UnknownCollection { id: String },
    BadCollectionFile { path: String, reason: String },
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
        Ok(t) => t,
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

    // Greedy + a window that fits the deepest rung — the probe is a diagnostic, so
    // the same (model, collection) must reproduce the same verdict run-to-run.
    let needed_ctx = opts.max_tokens.saturating_add(CTX_HEADROOM);
    let options = GenerateOptions { temperature: Some(0.0), num_ctx: Some(needed_ctx), ..Default::default() };

    let cancel = CancellationToken::new();
    let turn = BackendTurn {
        backend: opts.run.backend,
        endpoint: ep,
        model: opts.run.model.clone(),
        cancel: cancel.clone(),
        options: Some(options),
        keep_alive: None,
        is_thinking: false, // liveness probe at the answer floor, mirrors the GUI prompt path
        max_tokens: answer_tokens_for(Tier::Easy),
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };

    let ladder = build_ladder(opts.max_tokens, opts.steps);
    let mut on_rung = |done: usize, total: usize, point: &crate::inference::eval::cliff::CliffPoint| {
        let acc = point.composite.map(|c| format!("{:.0}%", c * 100.0)).unwrap_or_else(|| "—".into());
        eprintln!("· rung {done}/{total}: ~{} tok · {acc}", point.verified_tokens);
    };
    let mut no_step = |_s: crate::inference::eval::cliff::StepProgress| {};
    let report = run_cliff_with(
        &turn,
        &opts.run.model,
        &tasks,
        &opts.source,
        &ladder,
        &DEFAULT_DEPTHS,
        needed_ctx,
        &cancel,
        &mut on_rung,
        &mut no_step,
    )
    .await?;
    // `RunMode` is unused here (prompt-only probe) but kept on RunOptions for parity.
    let _ = RunMode::PromptBased;
    Ok(CliffOutcome::Probed(report))
}

//! `qm run` — run the built-in tool-calling suite against one model and produce a
//! Ready / Conditional / NotReady verdict. Pure of stdout/process concerns (the `qm`
//! bin renders + maps the exit code); this module only RUNS and ASSESSES.
//!
//! It mirrors the GUI's `commands/eval/batch_cmd.rs` minus Tauri: load the built-in
//! collection → `run_batch` (the thin, no-VRAM-gate entry) → `assess_report` (the
//! no-hardware verdict path). Nothing new in the eval engine — this is wiring.

pub mod config;
pub mod render;
pub mod sink;

pub use render::{exit_code, render_human, FailOn};

use crate::commands::doctor::probe::probe_backend;
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::{endpoint, remote_config};
use crate::inference::eval::agentic::difficulty::passk::max_tokens_for;
use crate::inference::eval::agentic::model_turn::BackendTurn;
use crate::inference::eval::agentic::runner::NUM_CTX_CEILING;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::batch::{run_batch, BatchSink};
use crate::inference::eval::readiness::profile;
use crate::inference::eval::readiness::types::{ModelVerdict, Readiness};
use crate::inference::eval::readiness::inputs::assess_report;
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::eval::toolcall::tasks::{builtin_collection, ToolTask};
use serde::Serialize;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// CLI-supplied options for one suite run.
pub struct RunOptions {
    pub backend: BackendKind,
    pub model: String,
    /// Built-in collection id (e.g. `easy-coding`).
    pub collection: String,
    /// Endpoint override (`--base`/`QM_BASE`); required for remote backends.
    pub base: Option<String>,
    /// Remote bearer credential (env/keychain, never argv).
    pub api_key: Option<String>,
    /// pass^k override (the strict all-k run count). `None` = the collection's tier default.
    pub k: Option<u32>,
    /// Reasoning model (raises the per-turn token budget + strips `<think>`).
    pub is_thinking: bool,
    /// Readiness profile id (a `profile::builtins()` id, e.g. `general-agent`).
    pub profile_id: String,
}

/// What a run produced. The bin maps each variant to the documented exit code.
pub enum RunOutcome {
    /// Backend didn't respond — exit 3 (an unreachable server is not a failing model).
    Unreachable { backend: BackendKind, endpoint: String },
    /// Reachable, but the requested model isn't served — exit 3.
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    /// Unknown built-in collection id — exit 2 (bad args).
    UnknownCollection { id: String },
    /// Unknown readiness profile id — exit 2 (bad args).
    UnknownProfile { id: String },
    /// The suite ran; carries the verdict(s).
    Ran(RunReport),
}

/// A completed run: the assessed verdict(s) for one model on one collection.
#[derive(Serialize, Clone, Debug)]
pub struct RunReport {
    pub collection_id: String,
    pub backend: BackendKind,
    pub model: String,
    pub profile_id: String,
    /// One row per measured path (prompt-based only in this first cut → one row).
    pub verdicts: Vec<ModelVerdict>,
}

impl RunReport {
    /// The worst status across measured paths — the honest headline (a model that is
    /// NotReady on any measured path is not Ready). `NotReady` when nothing measured.
    pub fn worst_status(&self) -> Readiness {
        self.verdicts
            .iter()
            .map(|v| v.verdict.status)
            .max_by_key(|s| match s {
                Readiness::Ready => 0,
                Readiness::Conditional => 1,
                Readiness::NotReady => 2,
            })
            .unwrap_or(Readiness::NotReady)
    }
}

/// The hardest agentic tier present — sizes the per-turn token budget. Mirrors
/// `batch_cmd::effective_tier` (private there); `Easy` when no task is agentic.
fn effective_tier(tasks: &[ToolTask]) -> Tier {
    tasks.iter().filter_map(|t| t.agentic.as_ref().map(|a| a.tier)).max().unwrap_or(Tier::Easy)
}

/// Run the built-in suite and assess it. Preflights reachability + model presence
/// (reusing the doctor probe) so an unreachable server or a missing model fails fast
/// with the right signal instead of being mislabelled a failing model.
pub async fn run_suite(opts: RunOptions) -> AppResult<RunOutcome> {
    let Some(mut tasks) = builtin_collection(&opts.collection) else {
        return Ok(RunOutcome::UnknownCollection { id: opts.collection });
    };
    let Some(profile) = profile::builtins().into_iter().find(|p| p.id == opts.profile_id) else {
        return Ok(RunOutcome::UnknownProfile { id: opts.profile_id });
    };

    // Preflight — reuse the doctor probe (reachability + served models).
    let probed = probe_backend(opts.backend, opts.base.as_deref(), Some(&opts.model), opts.api_key.as_deref()).await;
    if !probed.reachable {
        return Ok(RunOutcome::Unreachable { backend: opts.backend, endpoint: probed.endpoint });
    }
    if !probed.models.iter().any(|m| m == &opts.model) {
        return Ok(RunOutcome::ModelNotFound { backend: opts.backend, model: opts.model, available: probed.models });
    }

    // Apply the pass^k override to every agentic task, if the user set one.
    if let Some(k) = opts.k {
        for t in &mut tasks {
            if let Some(spec) = t.agentic.as_mut() {
                spec.k = Some(k);
            }
        }
    }

    // Resolve the endpoint the turn will hit; remote backends resolve their key from
    // remote_config, so seed it from --base + the env/keychain key.
    let ep = opts
        .base
        .clone()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| endpoint::base_url(opts.backend));
    match opts.backend {
        BackendKind::VLlm => remote_config::set_vllm(Some(ep.clone()), opts.api_key.clone()),
        BackendKind::SgLang => remote_config::set_sglang(Some(ep.clone()), opts.api_key.clone()),
        _ => {}
    }

    let tier = effective_tier(&tasks);
    let targets = vec![ModelTarget { model: opts.model.clone(), backend: opts.backend, is_thinking: opts.is_thinking }];
    let cancel = CancellationToken::new();
    let sink: Arc<dyn BatchSink> = Arc::new(sink::CliSink::new(tasks.len()));

    let turn_cancel = cancel.clone();
    let is_thinking = opts.is_thinking;
    let report = run_batch(&opts.collection, &targets, &tasks, cancel, sink, move |t: &ModelTarget| BackendTurn {
        backend: t.backend,
        endpoint: ep.clone(),
        model: t.model.clone(),
        cancel: turn_cancel.clone(),
        options: None,
        keep_alive: Some(600), // AGENTIC_KEEP_ALIVE_SECS — keep the model + KV cache resident across the task's many turns
        is_thinking,
        max_tokens: max_tokens_for(tier, is_thinking),
        cpu_offloaded: false,
        ctx_ceiling: NUM_CTX_CEILING,
        stop_cache: Default::default(),
    })
    .await?;

    let verdicts = assess_report(&report, &profile);
    Ok(RunOutcome::Ran(RunReport {
        collection_id: opts.collection,
        backend: opts.backend,
        model: opts.model,
        profile_id: opts.profile_id,
        verdicts,
    }))
}

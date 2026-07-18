//! `qm prompt` — a free-form generation: system + user prompt against a model with
//! inference params, streamed to stdout. The headless twin of the Workspace Run.
//! Reuses `run_prompt_inner` (Tauri-free; streams via an `on_token` closure) — the
//! same inference path the GUI's Run button drives, so the CLI and GUI agree.

use crate::cli::doctor::probe::probe_backend;
use crate::commands::prompt::prompt_options::to_generate_options;
use crate::commands::prompt::prompt_run::run_prompt_inner;
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::persistence::prompts::schema::InferenceParams;
use std::io::Write;
use tokio_util::sync::CancellationToken;

pub struct PromptOptions {
    pub backend: BackendKind,
    pub model: String,
    pub base: Option<String>,
    pub api_key: Option<String>,
    pub system: Option<String>,
    /// The user prompt (already resolved from `--user` or stdin by the bin).
    pub user: String,
    pub params: Option<InferenceParams>,
}

pub enum PromptOutcome {
    Unreachable { backend: BackendKind, endpoint: String },
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    /// Streamed to completion — token count for a trailing stderr summary.
    Done { tokens: u32 },
}

/// Preflight (reachable + model served), then stream tokens to stdout as they
/// arrive. Diagnostics stay on stderr (via the bin); this fn only writes tokens.
pub async fn run_prompt(opts: PromptOptions) -> AppResult<PromptOutcome> {
    let probed = probe_backend(opts.backend, opts.base.as_deref(), Some(&opts.model), opts.api_key.as_deref()).await;
    if !probed.reachable {
        return Ok(PromptOutcome::Unreachable { backend: opts.backend, endpoint: probed.endpoint });
    }
    if !probed.models.iter().any(|m| m == &opts.model) {
        return Ok(PromptOutcome::ModelNotFound { backend: opts.backend, model: opts.model, available: probed.models });
    }

    let ep = opts
        .base
        .clone()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| endpoint::base_url(opts.backend));

    let options = opts.params.as_ref().map(to_generate_options);
    let cancel = CancellationToken::new();
    // Stream each token straight to stdout, flushing so it appears live (the data
    // channel; the bin keeps all [QM-*] prose on stderr).
    let on_token = |t: &str| {
        print!("{t}");
        let _ = std::io::stdout().flush();
    };
    let stats = run_prompt_inner(
        opts.backend,
        &ep,
        &opts.model,
        &opts.user,
        opts.system.as_deref(),
        options,
        None,
        cancel,
        on_token,
    )
    .await?;
    println!(); // terminate the streamed line
    Ok(PromptOutcome::Done { tokens: stats.eval_count.unwrap_or(0) })
}

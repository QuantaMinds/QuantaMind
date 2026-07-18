//! `qm run` — run the built-in tool-calling suite against one model and produce a
//! Ready / Conditional / NotReady verdict. Pure of stdout/process concerns (the `qm`
//! bin renders + maps the exit code); this module only RUNS and ASSESSES.
//!
//! It mirrors the GUI's `commands/eval/batch_cmd.rs` minus Tauri: load the built-in
//! collection → the native-FC pass and/or the prompt pass → `assess_report` (the
//! no-hardware verdict path). Nothing new in the eval engine — this is wiring.

pub mod config;
pub mod junit;
pub mod render;
pub mod sink;

pub use junit::to_junit;
pub use render::{exit_code, render_human, render_scoreboard, FailOn};

use crate::cli::doctor::probe::probe_backend;
use crate::commands::eval::batch_cmd::probe_native_tools;
use crate::commands::remote::remote_health::{RemoteAuthReport, RemoteAuthStatus};
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::remote_guard::credential_allowed;
use crate::inference::backend::{endpoint, remote_config};
use crate::inference::eval::agentic::difficulty::passk::{max_tokens_for_preset, pass_k_for, ThinkPreset};
use crate::inference::eval::agentic::model_turn::{BackendTurn, NativeToolTurn};
use crate::inference::eval::agentic::runner::NUM_CTX_CEILING;
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::batch::{run_batch, run_native_fc_pass, AggAgentic, BatchColumn, BatchReport, BatchSink, NoVramGate};
use crate::inference::eval::readiness::inputs::assess_report;
use crate::inference::eval::readiness::profile;
use crate::inference::eval::readiness::types::{ModelVerdict, Readiness};
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::eval::toolcall::prompt::TerminalGuidance;
use crate::inference::eval::toolcall::tasks::{builtin_collection, ToolTask};
use crate::inference::ollama::ollama_show::probe_supports_thinking;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Which calling path(s) to exercise. `assess_report` emits one verdict row per
/// path that actually ran, so `Both` yields a native_fc and a prompt_based row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    PromptBased,
    Native,
    Both,
}

impl RunMode {
    fn wants_native(self) -> bool {
        matches!(self, RunMode::Native | RunMode::Both)
    }
    fn wants_prompt(self) -> bool {
        matches!(self, RunMode::PromptBased | RunMode::Both)
    }
}

/// CLI-supplied options for one suite run.
pub struct RunOptions {
    pub backend: BackendKind,
    pub model: String,
    pub collection: String,
    pub base: Option<String>,
    pub api_key: Option<String>,
    /// pass^k override (strict all-k). `None` = the collection tier's default.
    pub k: Option<u32>,
    /// Difficulty-tier override (scales the token budget + default k). `None` = the
    /// collection's own tier.
    pub tier: Option<Tier>,
    /// Reasoning-scratchpad preset: `Lean` (thinking off) / `Standard` / `Deep`.
    pub think: ThinkPreset,
    /// Native tool-calling, prompt-based, or both.
    pub mode: RunMode,
    /// Readiness profile id (a `profile::builtins()` id).
    pub profile_id: String,
}

/// What a run produced. The bin maps each variant to the documented exit code.
pub enum RunOutcome {
    Unreachable { backend: BackendKind, endpoint: String },
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    UnknownCollection { id: String },
    /// A `--collection` file that failed to read / parse / validate — exit 2.
    BadCollectionFile { path: String, reason: String },
    UnknownProfile { id: String },
    /// `--mode native` but the model/backend has no native tool-calling — exit 2.
    NativeUnsupported { backend: BackendKind, model: String },
    /// `--thinking standard|deep` but the model can't reason (Ollama 400s it) — exit 2.
    ThinkingUnsupported { backend: BackendKind, model: String },
    /// A remote backend responded but the credential didn't resolve `Ok` (401 / wrong
    /// path / server error) — a credential problem, NOT a missing model. Exit 3.
    CredentialError { backend: BackendKind, report: RemoteAuthReport },
    /// The run ERRORED — nothing could be measured (backend fault / 500 / timeout
    /// cascade). Exit 11 (retry), NOT a definitive NotReady: a measurement of nothing
    /// must never read as "your model failed".
    Inconclusive { reason: String },
    Ran(RunReport),
}

/// A completed run: the assessed verdict(s) for one model on one collection.
#[derive(Serialize, Clone, Debug)]
pub struct RunReport {
    pub collection_id: String,
    pub backend: BackendKind,
    pub model: String,
    pub profile_id: String,
    /// One row per measured path (native_fc and/or prompt_based).
    pub verdicts: Vec<ModelVerdict>,
}

impl RunReport {
    /// The worst status across measured paths — the honest headline. `NotReady`
    /// when nothing measured.
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

/// Will an OpenAI-compatible backend (llama.cpp / MLX / vLLM / SGLang) actually
/// produce reasoning in THIS model+server setup? Sends one tiny request and checks
/// for `reasoning_content` — the field llama.cpp (`--reasoning-format`) and vLLM use.
/// A null/absent field means `--thinking` would silently no-op (the exact bug this
/// catches). Fail-OPEN on any transport/parse error so a transient failure never
/// false-blocks a legitimate run.
async fn openai_reasons(ep: &str, model: &str, key: Option<&str>) -> bool {
    let Ok(c) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8)).build() else {
        return true;
    };
    let mut req = c.post(format!("{ep}/v1/chat/completions")).json(&serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply with just the number 7."}],
        "max_tokens": 32,
        "stream": false,
    }));
    if let Some(k) = key.filter(|k| !k.is_empty() && credential_allowed(ep)) {
        req = req.bearer_auth(k);
    }
    let Ok(resp) = req.send().await else { return true };
    // Fail-open on a non-2xx too: a transient 500/503 on the probe must not be read as
    // "this model can't reason" and false-block an otherwise-fine thinking run.
    if !resp.status().is_success() {
        return true;
    }
    let Ok(v) = resp.json::<serde_json::Value>().await else { return true };
    v.pointer("/choices/0/message/reasoning_content")
        .and_then(|r| r.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// The hardest agentic tier present — sizes the per-turn token budget. Mirrors
/// `batch_cmd::effective_tier` (private there); `Easy` when no task is agentic.
fn effective_tier(tasks: &[ToolTask]) -> Tier {
    tasks.iter().filter_map(|t| t.agentic.as_ref().map(|a| a.tier)).max().unwrap_or(Tier::Easy)
}

/// An empty per-target report the native pass fills / the prompt pass merges into.
/// Replica of `batch_cmd::skeleton_report` (private there).
fn skeleton(collection_id: &str, targets: &[ModelTarget]) -> BatchReport {
    BatchReport {
        collection_id: collection_id.to_string(),
        num_ctx: None,
        ollama_version: None,
        collection_hash: None,
        think_preset: None,
        columns: targets
            .iter()
            .map(|t| BatchColumn {
                model: t.model.clone(),
                backend: t.backend,
                toolcall: None,
                agentic: None,
                agentic_native_fc: None,
                error: None,
                is_thinking: t.is_thinking,
                cpu_offloaded: false,
                ctx_ceiling: None,
            })
            .collect(),
    }
}

/// Why a collection couldn't be resolved.
enum CollectionError {
    /// Not a file, and not a known built-in id.
    UnknownBuiltin,
    /// A file that failed to read/parse/validate (reason already redacted).
    BadFile(String),
}

/// Resolve `--collection`: a file path (JSON: a raw `ToolTask[]` or a v2 object,
/// auto-detected + size-capped by `evals::read_capped`) OR a built-in id. A spec that
/// names a file (has a separator or ends `.json`) always goes down the file path so a
/// typo'd filename reports a file error, not "unknown built-in".
fn load_collection(spec: &str) -> Result<Vec<ToolTask>, CollectionError> {
    let path = std::path::Path::new(spec);
    let looks_like_file = path.is_file() || spec.ends_with(".json") || spec.contains(std::path::MAIN_SEPARATOR) || spec.contains('/');
    if looks_like_file {
        return crate::persistence::evals::read_capped(path)
            .map_err(|e| CollectionError::BadFile(crate::redact::redact_path(&e.to_string())));
    }
    builtin_collection(spec).ok_or(CollectionError::UnknownBuiltin)
}

/// Run the built-in suite and assess it. Preflights reachability + model presence
/// (reusing the doctor probe) so an unreachable server or a missing model fails fast
/// with the right signal instead of being mislabelled a failing model.
pub async fn run_suite(opts: RunOptions) -> AppResult<RunOutcome> {
    let mut tasks = match load_collection(&opts.collection) {
        Ok(t) => t,
        Err(CollectionError::UnknownBuiltin) => return Ok(RunOutcome::UnknownCollection { id: opts.collection }),
        Err(CollectionError::BadFile(reason)) => {
            return Ok(RunOutcome::BadCollectionFile { path: opts.collection, reason })
        }
    };
    let Some(profile) = profile::builtins().into_iter().find(|p| p.id == opts.profile_id) else {
        return Ok(RunOutcome::UnknownProfile { id: opts.profile_id });
    };

    // Preflight — reuse the doctor probe (reachability + served models).
    let probed = probe_backend(opts.backend, opts.base.as_deref(), Some(&opts.model), opts.api_key.as_deref()).await;
    if !probed.reachable {
        return Ok(RunOutcome::Unreachable { backend: opts.backend, endpoint: probed.endpoint });
    }
    // A remote backend that responded but NOT with an OK credential (401 / wrong path /
    // server error) returns an empty model list — classify it as the credential problem
    // it is, not a bogus "model not found" (the status is right there in the probe).
    if let Some(cred) = probed.credential.filter(|c| c.status != RemoteAuthStatus::Ok) {
        return Ok(RunOutcome::CredentialError { backend: opts.backend, report: cred });
    }
    if !probed.models.iter().any(|m| m == &opts.model) {
        return Ok(RunOutcome::ModelNotFound { backend: opts.backend, model: opts.model, available: probed.models });
    }

    // Tier + pass^k overrides. A tier override also derives the tier's default k
    // (unless an explicit --k wins), mirroring `batch_cmd::apply_overrides`.
    for t in &mut tasks {
        if let Some(spec) = t.agentic.as_mut() {
            if let Some(tier) = opts.tier {
                spec.tier = tier;
            }
            if let Some(k) = opts.k {
                spec.k = Some(k);
            } else if let Some(tier) = opts.tier {
                spec.k = Some(pass_k_for(tier));
            }
        }
    }

    // Resolve the endpoint; remote backends resolve their key from remote_config.
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

    // `Lean` = reasoning off; `Standard`/`Deep` = on (raised token budget).
    let is_thinking = !matches!(opts.think, ThinkPreset::Lean);
    let preset = opts.think;

    // Guard: a Thinking-Budget preset where reasoning won't actually happen — shown
    // CLEARLY instead of silently no-op'ing (llama.cpp) or 400'ing every run into a
    // bogus verdict (Ollama). Ollama gates per-model (probe /api/show); the other
    // OpenAI-compatible backends can't be queried, so we probe the live setup for a
    // `reasoning_content` field.
    if is_thinking {
        let reasons = match opts.backend {
            BackendKind::Ollama => probe_supports_thinking(&ep, &opts.model).await,
            _ => openai_reasons(&ep, &opts.model, opts.api_key.as_deref()).await,
        };
        if !reasons {
            return Ok(RunOutcome::ThinkingUnsupported { backend: opts.backend, model: opts.model });
        }
    }
    let tier = opts.tier.unwrap_or_else(|| effective_tier(&tasks));
    let targets = vec![ModelTarget { model: opts.model.clone(), backend: opts.backend, is_thinking }];
    let cancel = CancellationToken::new();
    let sink: Arc<dyn BatchSink> = Arc::new(sink::CliSink::new(tasks.len()));

    // Native eligibility (Ollama /api/show tools; llama.cpp --jinja; MLX has none).
    let mut supported: HashSet<String> = HashSet::new();
    if opts.mode.wants_native() && probe_native_tools(opts.backend, &ep, &opts.model).await {
        supported.insert(opts.model.clone());
    }
    if opts.mode.wants_native() && supported.is_empty() && !opts.mode.wants_prompt() {
        return Ok(RunOutcome::NativeUnsupported { backend: opts.backend, model: opts.model });
    }

    // Native pass first (fills `agentic_native_fc` on a skeleton, collect the aggregates).
    let native_aggs: HashMap<String, AggAgentic> = if !supported.is_empty() {
        let mut skel = skeleton(&opts.collection, &targets);
        let ep_native = ep.clone();
        let backend = opts.backend;
        run_native_fc_pass(
            &mut skel,
            &tasks,
            &supported,
            cancel.clone(),
            move |model: &str, task: &ToolTask| {
                let terminal = match task.agentic.as_ref().map(|s| &s.end_state) {
                    Some(EndStateRule::RequireAll(_)) | Some(EndStateRule::RequireSequence(_)) => TerminalGuidance::MustUseTools,
                    _ => TerminalGuidance::PlainTextOk,
                };
                NativeToolTurn {
                    backend,
                    endpoint: ep_native.clone(),
                    model: model.to_string(),
                    tools: task.tools.clone(),
                    options: None,
                    terminal,
                    max_tokens: max_tokens_for_preset(tier, true, preset),
                    is_thinking,
                }
            },
            &[],
            &|_| {},
            &NoVramGate,
            sink.clone(),
        )
        .await?;
        skel.columns.into_iter().filter_map(|c| c.agentic_native_fc.map(|a| (c.model, a))).collect()
    } else {
        HashMap::new()
    };

    // Prompt pass (or the skeleton when native-only).
    let mut report = if opts.mode.wants_prompt() {
        let turn_cancel = cancel.clone();
        let ep_prompt = ep.clone();
        run_batch(&opts.collection, &targets, &tasks, cancel, sink, move |t: &ModelTarget| BackendTurn {
            backend: t.backend,
            endpoint: ep_prompt.clone(),
            model: t.model.clone(),
            cancel: turn_cancel.clone(),
            options: None,
            keep_alive: Some(600),
            is_thinking: t.is_thinking,
            max_tokens: max_tokens_for_preset(tier, t.is_thinking, preset),
            cpu_offloaded: false,
            ctx_ceiling: NUM_CTX_CEILING,
            stop_cache: Default::default(),
        })
        .await?
    } else {
        skeleton(&opts.collection, &targets)
    };

    // Merge the native aggregates into the report columns.
    for col in &mut report.columns {
        if let Some(a) = native_aggs.get(&col.model) {
            col.agentic_native_fc = Some(a.clone());
        }
    }
    report.think_preset = Some(preset);

    // A run that ERRORED couldn't measure anything → Inconclusive (retry), NOT a
    // NotReady verdict. A column error is the loud signal; surface it redacted (rule
    // 7f) instead of leaking a raw internal error string into a readiness "reason".
    if let Some(err) = report.columns.iter().find_map(|c| c.error.as_deref()) {
        return Ok(RunOutcome::Inconclusive { reason: crate::redact::redact_path(err) });
    }

    let verdicts = assess_report(&report, &profile);
    // Belt-and-suspenders: even with no column error, zero measured trials across every
    // path means we measured nothing — Inconclusive, not a fabricated NotReady. (A real
    // failure has total_runs > 0: the model ran and lost.)
    let trials: Vec<u32> = verdicts.iter().map(|v| v.total_runs).collect();
    if render::measured_nothing(&trials) {
        return Ok(RunOutcome::Inconclusive { reason: "the run produced no measured trials".into() });
    }

    Ok(RunOutcome::Ran(RunReport {
        // Display the collection by its basename, never the full path (rule 7f — no
        // absolute path / machine info in output).
        collection_id: display_collection(&opts.collection),
        backend: opts.backend,
        model: opts.model,
        profile_id: opts.profile_id,
        verdicts,
    }))
}

/// A leak-safe display name for a collection: a built-in id as-is, a file as its
/// basename only (never the absolute path).
fn display_collection(spec: &str) -> String {
    let path = std::path::Path::new(spec);
    if spec.ends_with(".json") || spec.contains('/') || spec.contains(std::path::MAIN_SEPARATOR) {
        return path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| spec.to_string());
    }
    spec.to_string()
}

#[cfg(test)]
mod tests {
    use super::display_collection;

    #[test]
    fn display_collection_never_leaks_an_absolute_path() {
        assert_eq!(display_collection("easy-coding"), "easy-coding"); // built-in id as-is
        assert_eq!(display_collection("/private/tmp/abc/my_suite.json"), "my_suite.json");
        assert_eq!(display_collection("./rel/dir/x.json"), "x.json");
        // No username / home path survives (rule 7f).
        let d = display_collection("/Users/alice/secret/col.json");
        assert_eq!(d, "col.json");
        assert!(!d.contains("alice") && !d.contains("secret"));
    }
}

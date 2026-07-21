//! `qm run` — run the built-in tool-calling suite against one model and produce a
//! Ready / Conditional / NotReady verdict. Pure of stdout/process concerns (the `qm`
//! bin renders + maps the exit code); this module only RUNS and ASSESSES.
//!
//! It mirrors the GUI's `commands/eval/batch_cmd.rs` minus Tauri: load the built-in
//! collection → the native-FC pass and/or the prompt pass → `assess_report` (the
//! no-hardware verdict path). Nothing new in the eval engine — this is wiring.

pub mod config;
pub mod costs;
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
    /// Readiness profile: a built-in id OR a `.json` file path.
    pub profile_id: String,
    /// If set, write the raw `BatchReport` here (for offline `qm report --report`).
    pub save_report: Option<std::path::PathBuf>,
    /// Per-turn step cap override (UI "Max Steps"). `None` = each task's authored cap.
    pub max_steps: Option<u32>,
    /// Decoy-tool count injected per task (UI "Decoy Tools"). `None` = the task's own.
    pub decoy_tools: Option<u32>,
    /// Global inference params (temperature/top_p/…). `None` = greedy eval default.
    /// When set, the header options win over the greedy spec default (see
    /// `merge_eval_options`), so `qm run --temperature 0.7` matches the GUI's
    /// "run with my params" behavior.
    pub params: Option<crate::persistence::prompts::schema::InferenceParams>,
    /// Capture + report per-task run costs (prefill/decode split, thinking split,
    /// cache hits, peak context, step-end RSS, KV-at-peak) — the CLI twin of the
    /// app's Latency Test-run view. Off by default: it samples host RSS per turn.
    pub costs: bool,
}

/// What a run produced. The bin maps each variant to the documented exit code.
pub enum RunOutcome {
    Unreachable { backend: BackendKind, endpoint: String },
    ModelNotFound { backend: BackendKind, model: String, available: Vec<String> },
    UnknownCollection { id: String },
    /// A `--collection` file that failed to read / parse / validate — exit 2.
    BadCollectionFile { path: String, reason: String },
    UnknownProfile { id: String },
    /// A `--profile` file that failed to read / parse — exit 2.
    BadProfileFile { path: String, reason: String },
    /// `--mode native` but the model/backend has no native tool-calling — exit 2.
    NativeUnsupported { backend: BackendKind, model: String },
    /// `--thinking standard|deep` but the model can't reason (Ollama 400s it) — exit 2.
    ThinkingUnsupported { backend: BackendKind, model: String },
    /// An uploaded collection FAILED the mandatory validation gate — testing must not
    /// start on a broken answer key. Exit 20; `findings` name every defect + its fix.
    CollectionInvalid { findings: Vec<String> },
    /// World tasks present but npx/sqlite3 missing — exit 2 with the install fix,
    /// BEFORE any model time is burnt.
    WorldDepsMissing { fix: String },
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
    /// Per-task run costs — present only when `--costs` captured them. Omitted from
    /// JSON entirely otherwise (absent ≠ an empty measurement).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub costs: Option<costs::RunCosts>,
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
                is_thinking: t.is_thinking,
                ..Default::default()
            })
            .collect(),
    }
}

/// Why a collection couldn't be resolved. `pub(crate)` — shared with `cli::cliff`.
#[derive(Debug)]
pub(crate) enum CollectionError {
    /// Not a file, and not a known built-in id.
    UnknownBuiltin,
    /// A file that failed to read/parse/validate (reason already redacted).
    BadFile(String),
}

/// A resolved collection + whether it came from a user FILE (uploaded/authored) —
/// file collections must pass the full validation gate before any model runs.
pub(crate) struct LoadedCollection {
    pub tasks: Vec<ToolTask>,
    pub from_file: bool,
}

/// Resolve `--collection`: a file path OR a built-in id. A file is auto-detected
/// across THREE JSON shapes (all size-capped at 1 MB):
/// 1. a v2 collection object `{name, tier, tasks: […]}`;
/// 2. a raw `ToolTask[]` array (may carry `agentic.mcp` worlds);
/// 3. a WORLD file — an array of `{name, instruction, world:{type:fs|db,…}, oracle}`
///    (the same shape the desktop MCP builder authors), converted via the existing
///    `build_mcp_tasks`.
/// A spec that names a file (separator or `.json`) always goes down the file path so
/// a typo'd filename reports a file error, not "unknown built-in".
pub(crate) fn load_collection(spec: &str) -> Result<LoadedCollection, CollectionError> {
    let path = std::path::Path::new(spec);
    let looks_like_file = path.is_file() || spec.ends_with(".json") || spec.contains(std::path::MAIN_SEPARATOR) || spec.contains('/');
    if looks_like_file {
        let text = crate::persistence::evals::read_text_capped(path)
            .map_err(|e| CollectionError::BadFile(crate::redact::redact_path(&e.to_string())))?;
        // World-file shape: an array whose items carry `instruction` + `world`.
        let is_world_file = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.as_array().and_then(|a| a.first().cloned()))
            .map(|first| first.get("instruction").is_some() && first.get("world").is_some())
            .unwrap_or(false);
        let tasks = if is_world_file {
            let specs: Vec<crate::commands::mcp::run_cmd::McpTaskSpec> = serde_json::from_str(&text)
                .map_err(|e| CollectionError::BadFile(format!("world file: {e}")))?;
            crate::commands::mcp::task_cmd::build_mcp_tasks(specs)
                .map_err(|e| CollectionError::BadFile(crate::redact::redact_path(&e.to_string())))?
        } else {
            crate::persistence::evals::parse_collection(&text)
                .map_err(|e| CollectionError::BadFile(crate::redact::redact_path(&e.to_string())))?
        };
        return Ok(LoadedCollection { tasks, from_file: true });
    }
    builtin_collection(spec)
        .map(|tasks| LoadedCollection { tasks, from_file: false })
        .ok_or(CollectionError::UnknownBuiltin)
}

/// Why a profile couldn't be resolved.
enum ProfileError {
    UnknownBuiltin,
    BadFile(String),
}

/// Resolve `--profile`: a JSON file (a `ReadinessProfile`, 1 MB cap) OR a built-in id
/// (`general-agent` / `rag-assistant` / `coding-agent`).
fn load_profile(spec: &str) -> Result<crate::inference::eval::readiness::profile::ReadinessProfile, ProfileError> {
    let path = std::path::Path::new(spec);
    let looks_like_file = path.is_file() || spec.ends_with(".json") || spec.contains('/') || spec.contains(std::path::MAIN_SEPARATOR);
    if looks_like_file {
        let text = crate::persistence::evals::read_text_capped(path)
            .map_err(|e| ProfileError::BadFile(crate::redact::redact_path(&e.to_string())))?;
        return parse_profile_lenient(&text).map_err(ProfileError::BadFile);
    }
    profile::builtins().into_iter().find(|p| p.id == spec).ok_or(ProfileError::UnknownBuiltin)
}

/// Deserialize a `ReadinessProfile`, accepting a `required_tier` written in any case
/// (`Easy`/`EASY`/`easy`) — the built-in v2 *collection* loader is case-insensitive
/// about tier, so a hand-written profile shouldn't be stricter (a real papercut).
fn parse_profile_lenient(text: &str) -> Result<crate::inference::eval::readiness::profile::ReadinessProfile, String> {
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if let Some(lowered) = value.get("required_tier").and_then(|v| v.as_str()).map(str::to_lowercase) {
        value["required_tier"] = serde_json::Value::String(lowered);
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Run the built-in suite and assess it. Preflights reachability + model presence
/// (reusing the doctor probe) so an unreachable server or a missing model fails fast
/// with the right signal instead of being mislabelled a failing model.
pub async fn run_suite(opts: RunOptions) -> AppResult<RunOutcome> {
    let loaded = match load_collection(&opts.collection) {
        Ok(t) => t,
        Err(CollectionError::UnknownBuiltin) => return Ok(RunOutcome::UnknownCollection { id: opts.collection }),
        Err(CollectionError::BadFile(reason)) => {
            return Ok(RunOutcome::BadCollectionFile { path: opts.collection, reason })
        }
    };
    let mut tasks = loaded.tasks;

    // MANDATORY validate-before-run gate for uploaded/user files (built-ins are
    // CI-guarded at authoring time): an invalid answer key would make every pass^k a
    // lie, so testing MUST NOT start on one. No bypass flag.
    if loaded.from_file {
        let specs: Vec<&crate::inference::eval::mcp::world::McpSpec> =
            tasks.iter().filter_map(|t| t.agentic.as_ref().and_then(|a| a.mcp.as_ref())).collect();
        if let Some(fix) = crate::inference::eval::mcp::validate::world_deps_missing(&specs) {
            return Ok(RunOutcome::WorldDepsMissing { fix });
        }
        let mut v = crate::inference::eval::agentic::v2::oracle::validate_collection_deep(&tasks).await;
        crate::inference::eval::mcp::validate::merge_world_checks(&mut v, &tasks, true).await;
        if !v.ok {
            let mut findings = Vec::new();
            if let Some(e) = &v.structural_error {
                findings.push(e.clone());
            }
            for t in &v.tasks {
                if t.reachable == "no" {
                    findings.push(format!("{}: unreachable — {}", t.id, t.detail));
                }
                if t.discriminating == Some(false) {
                    findings.push(format!("{}: not discriminating — a do-nothing agent passes it", t.id));
                }
                for f in &t.semantic {
                    findings.push(format!("{}: {f}", t.id));
                }
            }
            return Ok(RunOutcome::CollectionInvalid { findings });
        }
        // Warnings never block, but the user must see them before the run.
        for t in &v.tasks {
            for w in &t.semantic_warnings {
                eprintln!("! {}: {w}", t.id);
            }
        }
    }
    let profile = match load_profile(&opts.profile_id) {
        Ok(p) => p,
        Err(ProfileError::UnknownBuiltin) => return Ok(RunOutcome::UnknownProfile { id: opts.profile_id }),
        Err(ProfileError::BadFile(reason)) => return Ok(RunOutcome::BadProfileFile { path: opts.profile_id, reason }),
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
            // Max-steps + decoy overrides — same field targets as `apply_overrides`.
            if opts.max_steps.is_some() {
                spec.max_steps = opts.max_steps;
            }
            if let Some(n) = opts.decoy_tools {
                spec.axes.get_or_insert_with(Default::default).decoy_tools = n;
            }
        }
    }

    // Global inference params → the header options every turn merges over the greedy
    // spec default (temperature/top_p/top_k win; see `merge_eval_options`). `None`
    // keeps the reproducible greedy eval behavior.
    let turn_options = opts.params.as_ref().map(crate::commands::prompt::prompt_options::to_generate_options);

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
    // Kept concrete so `--costs` can read the captured turns back after the run.
    let cli_sink = Arc::new(if opts.costs {
        sink::CliSink::capturing(tasks.len(), opts.backend)
    } else {
        sink::CliSink::new(tasks.len())
    });
    let sink: Arc<dyn BatchSink> = cli_sink.clone();

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
        let native_options = turn_options.clone();
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
                    options: native_options.clone(),
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
        let prompt_options = turn_options.clone();
        run_batch(&opts.collection, &targets, &tasks, cancel, sink, move |t: &ModelTarget| BackendTurn {
            backend: t.backend,
            endpoint: ep_prompt.clone(),
            model: t.model.clone(),
            cancel: turn_cancel.clone(),
            options: prompt_options.clone(),
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
    // Stamp the measured placement facts via the SHARED helper — the same one the GUI's
    // batch command uses, so `qm --costs` and the app's Latency view can never drift.
    // (llama-server launch facts stay unstamped here: the CLI never spawns servers, and
    // an externally managed server's flags are unknowable — never guessed.)
    let ep_probe = ep.clone();
    let placements = crate::inference::eval::run_facts::probe_placements(&targets, move |_| ep_probe.clone()).await;
    crate::inference::eval::run_facts::stamp_placements(&mut report.columns, &placements);

    // Persist the raw BatchReport if asked (for offline `qm report --report`). Saved
    // even for an errored/inconclusive run — the raw evidence is still worth keeping.
    if let Some(path) = &opts.save_report {
        let json = serde_json::to_string_pretty(&report).map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| crate::errors::AppError::Internal(format!("write report: {e}")))?;
    }

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

    // Per-task run costs (`--costs`): assemble from the sink's captured turns + the
    // stamped column, with KV-at-peak from the model's dims (Ollama /api/show; other
    // backends can't be dim-probed by model name → the KV figure stays None, honestly).
    let costs = if opts.costs {
        let dims = match opts.backend {
            BackendKind::Ollama => crate::commands::models::model_inspect::fetch_dims(&opts.model)
                .await
                .map(|d| (d.layers, d.head_count, d.head_count_kv, d.embedding_length, d.kv_estimated)),
            _ => None,
        };
        let column = report.columns.iter().find(|c| c.model == opts.model);
        Some(costs::assemble(&opts.model, &cli_sink.captured_steps(), &cli_sink.captured_outcomes(), column, dims))
    } else {
        None
    };

    Ok(RunOutcome::Ran(RunReport {
        // Display the collection by its basename, never the full path (rule 7f — no
        // absolute path / machine info in output).
        collection_id: display_collection(&opts.collection),
        backend: opts.backend,
        model: opts.model,
        profile_id: opts.profile_id,
        verdicts,
        costs,
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

/// Outcome of an offline `qm report --report <file>` re-assessment.
pub enum ReportOutcome {
    BadReportFile { path: String, reason: String },
    UnknownProfile { id: String },
    BadProfileFile { path: String, reason: String },
    Ran(RunReport),
}

/// Offline: reload a saved `BatchReport` (written by `run`/`test --save-report`) and
/// re-assess it against a profile (a built-in id OR a `.json` file) — no backend, no
/// inference. Lets you score one run against many bars.
pub fn assess_saved(report_path: &str, profile_spec: &str) -> ReportOutcome {
    let text = match crate::persistence::evals::read_text_capped(std::path::Path::new(report_path)) {
        Ok(t) => t,
        Err(e) => {
            return ReportOutcome::BadReportFile {
                path: report_path.to_string(),
                reason: crate::redact::redact_path(&e.to_string()),
            }
        }
    };
    let report: BatchReport = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => return ReportOutcome::BadReportFile { path: report_path.to_string(), reason: e.to_string() },
    };
    let profile = match load_profile(profile_spec) {
        Ok(p) => p,
        Err(ProfileError::UnknownBuiltin) => return ReportOutcome::UnknownProfile { id: profile_spec.to_string() },
        Err(ProfileError::BadFile(reason)) => return ReportOutcome::BadProfileFile { path: profile_spec.to_string(), reason },
    };
    let verdicts = assess_report(&report, &profile);
    let first = report.columns.first();
    ReportOutcome::Ran(RunReport {
        collection_id: display_collection(&report.collection_id),
        backend: first.map(|c| c.backend).unwrap_or_default(),
        model: first.map(|c| c.model.clone()).unwrap_or_default(),
        profile_id: profile.id.clone(),
        verdicts,
        costs: None, // an offline re-assessment has no captured turns to cost
    })
}

#[cfg(test)]
mod tests {
    use super::{display_collection, parse_profile_lenient};

    #[test]
    fn profile_tier_is_case_insensitive_like_collections() {
        let base = r#"{"id":"s","name":"S","min_pass_k":0.9,"max_avg_steps":null,
            "max_ms_per_step":null,"min_context_tokens":null,"forbid_infinite_loop":true,
            "forbid_hallucinated_completion":true,"require_full_vram":false,
            "require_native_fc":false,"required_tier":"TIER"}"#;
        // "Easy" / "EASY" / "easy" all parse to the same profile.
        for t in ["Easy", "EASY", "easy", "Medium"] {
            let p = parse_profile_lenient(&base.replace("TIER", t)).unwrap_or_else(|e| panic!("tier {t}: {e}"));
            assert_eq!(p.min_pass_k, 0.9);
        }
        // A genuinely invalid tier still errors (clearly).
        assert!(parse_profile_lenient(&base.replace("TIER", "gigantic")).is_err());
    }

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

use crate::commands::emit::log_emit;
use crate::commands::eval::batch_payloads::{
    AgenticStepPayload, BatchCompletePayload, BatchProgress, EVENT_AGENTIC_STEP, EVENT_BATCH_COMPLETE,
    EVENT_BATCH_PROGRESS,
};
use crate::commands::eval::toolcall_cmd::endpoint_for;
use crate::commands::prompt::prompt_options::{to_generate_options, validate_params};
use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::difficulty::passk::{max_tokens_for_preset, pass_k_for, ThinkPreset};
use crate::inference::eval::agentic::model_turn::{BackendTurn, NativeToolTurn};
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::toolcall::prompt::TerminalGuidance;
use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::agentic::v2::collection::load_v2_collection;
use crate::inference::eval::agentic::v2::scenarios::{collection_hash, v2_json};
use crate::inference::eval::batch::{
    batch_summaries, fold_report, run_batch_resumable, run_native_fc_pass, AggAgentic, BatchColumn, BatchReport,
    BatchSink, CompletedUnit, OllamaVramGate, TaskOutcome,
};
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::eval::toolcall::tasks::{validate_tasks, ToolTask};
use crate::commands::system::hardware::snapshot;
use crate::commands::system::process_memory;
use crate::inference::eval::readiness::hardware::hwclass::agentic_ctx_ceiling;
use crate::inference::llama::llama::probe_llama_n_ctx;
use crate::commands::llama::llama_server_types::LlamaServerState;
use crate::inference::eval::run_facts;
use crate::inference::ollama::ollama_show::probe_ollama_version;
use crate::persistence::eval_history;
use crate::persistence::jobs::queue::{self, RunConfig};
use crate::persistence::jobs::transcripts;
use crate::persistence::prompts::schema::InferenceParams;
use crate::persistence::readiness::reports;
use crate::sync::MutexExt;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

/// Where the resumable job logs live (`app_config_dir/jobs/<run_id>.jsonl`).
fn jobs_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("jobs"))
}

/// Per-collection regression log dir (shared with the matrix command).
fn history_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("history"))
}

/// Per-(model, task) agentic transcript dir, namespaced so agentic traces never
/// co-mingle with any other transcript store.
fn transcripts_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("agentic_transcripts"))
}

/// Where the last full batch report per collection is persisted — Rust's source
/// of truth for the readiness verdict (the Agent Report page + future CLI read it).
fn reports_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("batch_reports"))
}

/// Run-level cancellation for the batch dispatcher (mirrors `CompareRunState`).
#[derive(Default)]
pub struct BatchRunState {
    cancel: Mutex<Option<CancellationToken>>,
}

impl BatchRunState {
    /// Register a fresh cancellation token for a new run, cancelling any prior one, and
    /// return it. Shared so other run commands (e.g. the MCP BYO adapter) are stopped by
    /// the SAME `stop_batch_eval` button.
    pub fn begin(&self) -> CancellationToken {
        let cancel = CancellationToken::new();
        let mut g = self.cancel.lock_recover();
        if let Some(prev) = g.take() {
            prev.cancel();
        }
        *g = Some(cancel.clone());
        cancel
    }
}

/// Bridges domain batch events onto Tauri events — the single place the batch
/// payload shapes meet the IPC layer (see `docs/architecture.md#layering`).
/// Also persists each agentic turn to the on-disk transcript
/// (`agentic_transcripts/`, latest batch only) so a failing run can be
/// post-mortemed after the fact — the live event stream is the UI's copy, the
/// transcript is the durable one. Writes are best-effort: a disk hiccup must
/// never kill a batch, but it is logged loudly, never swallowed silently.
struct TauriBatchSink {
    app: AppHandle,
    collection_id: String,
    /// `None` when `app_config_dir` was unavailable at construction — the run
    /// proceeds without transcripts (warned once at construction).
    transcripts_dir: Option<PathBuf>,
    /// model → backend, so each streamed step can carry a host RSS sample of the RIGHT
    /// local server process (the generation layer stays ID-free and host-free; sampling
    /// lives here at the command boundary). Remote/unknown → the sample stays `None`.
    backends: HashMap<String, BackendKind>,
}

impl TauriBatchSink {
    fn transcript(&self, model: &str, task_id: &str, native: bool) -> Option<PathBuf> {
        let dir = self.transcripts_dir.as_ref()?;
        Some(transcripts::transcript_path(dir, &self.collection_id, model, task_id, native))
    }
    fn warn_write(model: &str, task_id: &str, e: crate::errors::AppError) {
        println!("[batch] WARN: transcript write failed for {model}/{task_id}: {e} — run continues, transcript incomplete");
    }
}

impl BatchSink for TauriBatchSink {
    fn task_started(&self, model: &str, task_id: &str, index: usize, total: usize, category: &str, is_native: bool) {
        log_emit(&self.app, EVENT_BATCH_PROGRESS, BatchProgress::Started {
            collection_id: self.collection_id.clone(),
            model: model.into(), task_id: task_id.into(), index, total, category: category.into(), is_native,
        });
        if let Some(path) = self.transcript(model, task_id, is_native) {
            if let Err(e) = transcripts::begin_task(&path) {
                Self::warn_write(model, task_id, e);
            }
        }
    }
    fn agentic_turn(&self, model: &str, task_id: &str, step: &TrajectoryStep, is_native: bool) {
        // Step-END host sample: whole-process RSS of the local inference server (weights +
        // residue — never a per-task delta; see the field's contract on `TrajectoryStep`).
        let mut step = step.clone();
        if step.resident_bytes.is_none() {
            step.resident_bytes = self.backends.get(model).copied().and_then(process_memory::backend_rss);
        }
        log_emit(&self.app, EVENT_AGENTIC_STEP, AgenticStepPayload {
            collection_id: self.collection_id.clone(),
            model: model.into(), task_id: task_id.into(), step: step.clone(), is_native,
        });
        if let Some(path) = self.transcript(model, task_id, is_native) {
            if let Err(e) = transcripts::append_step(&path, &step) {
                Self::warn_write(model, task_id, e);
            }
        }
    }
    fn task_done(&self, model: &str, task_id: &str, outcome: &TaskOutcome, is_native: bool) {
        log_emit(&self.app, EVENT_BATCH_PROGRESS, BatchProgress::Done {
            collection_id: self.collection_id.clone(),
            model: model.into(), task_id: task_id.into(), outcome: outcome.clone(), is_native,
        });
        if let Some(path) = self.transcript(model, task_id, is_native) {
            if let Err(e) = transcripts::append_outcome(&path, outcome) {
                Self::warn_write(model, task_id, e);
            }
        }
    }
}

/// An empty report with one column per target (all metrics `None`). The native pass runs FIRST
/// and fills `agentic_native_fc` on this skeleton; the prompt pass then produces the real report
/// and we merge the native aggregates in. Lets the native column surface before the prompt pass.
fn skeleton_report(collection_id: &str, targets: &[ModelTarget]) -> BatchReport {
    BatchReport {
        collection_id: collection_id.to_string(),
        num_ctx: None,
        ollama_version: None,
        collection_hash: None, // set on the FINAL report only (content-verified); intermediates stay unpublishable
        think_preset: None,    // stamped on the final report
        params: None,          // stamped on the final report
        columns: targets
            .iter()
            .map(|t| BatchColumn {
                model: t.model.clone(),
                backend: t.backend,
                is_thinking: t.is_thinking,
                ..Default::default() // reports + placement/config facts stamped on the final report
            })
            .collect(),
    }
}

/// The leaderboard identity hash for a run — CONTENT-VERIFIED (the fork-on-edit guard).
/// `Some(collection_hash(id))` ONLY when `tasks` are byte-for-byte the pristine bundled collection
/// (compared as `serde_json::Value`, exact — no tolerance); `None` for a custom/imported id OR ANY
/// edit to ANY field (world_state, end_state, prompt, tools, …). Compared on the RECEIVED tasks
/// (pre-`apply_overrides`) so run params (k/tier/decoys, passed separately) never fork the identity.
/// This is the single source of truth for publishability — publish reads the report's hash, never
/// re-derives from `collection_id`.
fn verified_collection_hash(collection_id: &str, tasks: &[ToolTask]) -> Option<String> {
    let pristine = v2_json(collection_id).and_then(|j| load_v2_collection(j).ok())?;
    let received = serde_json::to_value(tasks).ok()?;
    let pristine_v = serde_json::to_value(&pristine).ok()?;
    if received == pristine_v {
        collection_hash(collection_id)
    } else {
        None
    }
}

/// Apply the run-time difficulty / K / Max-Steps / decoy overrides to every agentic
/// task — the UI controls override the persisted per-task spec. Non-agentic tasks are
/// untouched.
///
/// `tier` set (a chosen tier or `Auto`) stamps `spec.tier` and, when the UI sends no
/// explicit `k`, derives the locked Pass^k via `pass_k_for(tier)` and stamps it onto
/// the spec so the run matches the locked display exactly (an authored per-task `k`
/// no longer silently wins). An explicit `k` (the `Custom` escape hatch) always wins.
/// `decoy_tools` set rewrites each spec's `axes.decoy_tools`; `None` leaves the
/// task-authored decoys intact.
/// The hardest tier among the agentic tasks — the source of truth for the tier-scaled
/// thinking token budget. Reads each task's own `agentic.tier` (set by authoring or by
/// `apply_overrides`), the SAME source `sandbox_for` uses for `max_steps`, so the budget can
/// never disagree with the step budget. Robust to the UI tier being `Auto`/unset (which would
/// otherwise default Hard/Medium/Extreme tasks to Easy's cap). The hardest tier present is a
/// safe choice for a mixed collection: `num_predict` is only a cap, so an easier task is
/// never harmed by a larger ceiling. `Easy` when no task carries an agentic tier.
fn effective_tier(tasks: &[ToolTask]) -> Tier {
    tasks.iter().filter_map(|t| t.agentic.as_ref().map(|a| a.tier)).max().unwrap_or(Tier::Easy)
}

fn apply_overrides(
    mut tasks: Vec<ToolTask>,
    k: Option<u32>,
    max_steps: Option<u32>,
    tier: Option<Tier>,
    decoy_tools: Option<u32>,
) -> Vec<ToolTask> {
    for t in &mut tasks {
        if let Some(spec) = t.agentic.as_mut() {
            if let Some(tier) = tier {
                spec.tier = tier;
            }
            // Explicit UI `k` wins; otherwise a chosen tier derives the locked `k`.
            if let Some(k) = k {
                spec.k = Some(k);
            } else if let Some(tier) = tier {
                spec.k = Some(pass_k_for(tier));
            }
            if max_steps.is_some() {
                spec.max_steps = max_steps;
            }
            if let Some(n) = decoy_tools {
                spec.axes.get_or_insert_with(Default::default).decoy_tools = n;
            }
        }
    }
    tasks
}

/// Floor (seconds) the eval batch pins the model resident for. An agentic task fires
/// ~k × max_steps sequential generate calls; with `keep_alive` unset Ollama's default
/// 5-min idle unload can fire across an inter-task/inter-turn gap, evicting the model
/// AND its prefix-KV cache mid-run (a cold reload then charges as a stall). This floor
/// keeps `warm_up()` (which honors the same field) pinned across the whole batch.
const AGENTIC_KEEP_ALIVE_SECS: i32 = 600;

/// The batch `keep_alive`: an explicit UI value (incl. `-1` = forever, or a smaller
/// override) always wins; otherwise apply the resident floor so the cache survives.
fn agentic_keep_alive(configured: Option<i32>) -> Option<i32> {
    configured.or(Some(AGENTIC_KEEP_ALIVE_SECS))
}

/// The single streaming eval command: validate, write the resumable job-queue
/// header, then run the prompt (+ optional native) passes as a crash-resumable
/// queue with the VRAM-isolation gate. Crosses the IPC boundary once.
#[tauri::command]
pub async fn run_batch_eval(
    app: AppHandle,
    state: tauri::State<'_, BatchRunState>,
    collection_id: String,
    targets: Vec<ModelTarget>,
    tasks: Vec<ToolTask>,
    k: Option<u32>,
    max_steps: Option<u32>,
    params: Option<InferenceParams>,
    keep_alive: Option<i32>,
    run_native_fc: Option<bool>,
    tier: Option<Tier>,
    decoy_tools: Option<u32>,
    run_prompt_based: Option<bool>,
    think_preset: Option<ThinkPreset>,
    eval_concurrency: Option<usize>,
) -> Result<BatchReport, AppError> {
    validate_tasks(&tasks)?;
    if let Some(p) = &params {
        validate_params(p)?;
    }
    let config = RunConfig {
        collection_id: collection_id.clone(),
        targets,
        tasks,
        k,
        max_steps,
        params,
        keep_alive,
        native: run_native_fc.unwrap_or(false),
        // Default true: an old frontend / persisted job without the flag keeps the prior behavior
        // where the prompt pass always ran.
        prompt: run_prompt_based.unwrap_or(true),
        tier,
        decoy_tools,
        think_preset: think_preset.unwrap_or_default(),
        // PR2: the inner task-concurrency knob. `None` (default) → serial (N=1), byte-identical to
        // the pre-concurrency dispatcher. Not surfaced to qm users yet (PR4 owns --parallel).
        eval_concurrency,
    };
    // Start a fresh job log (header only) — a leftover log means an interrupted run.
    queue::create(&queue::run_path(&jobs_dir(&app)?, &collection_id), &config)?;
    run_passes(&app, &state, &config, &[]).await
}

/// Run the prompt + optional native passes for `config`, resuming over `prior`
/// completed units and appending every new unit to the job log. Shared by a fresh
/// run (`prior = &[]`) and `resume_batch_eval`. On success: **transactional finish**
/// — save the report, verify it persisted, and only THEN delete the recovery log.
/// A VRAM-gate `Err` propagates (halts) with the log intact for a later resume.
pub(crate) async fn run_passes(
    app: &AppHandle,
    state: &tauri::State<'_, BatchRunState>,
    config: &RunConfig,
    prior: &[CompletedUnit],
) -> Result<BatchReport, AppError> {
    let options = config.params.as_ref().map(to_generate_options);
    let cancel = CancellationToken::new();
    {
        let mut g = state.cancel.lock_recover();
        if let Some(prev) = g.take() {
            prev.cancel();
        }
        *g = Some(cancel.clone());
    }
    let tasks = apply_overrides(config.tasks.clone(), config.k, config.max_steps, config.tier, config.decoy_tools);
    // Inner task-concurrency (PR2): `None`/`0`/`1` all resolve to serial. Threaded into both
    // dispatchers AND used as the per-task `budget_scale` (an N-task batch inflates each task's
    // wall-clock ~N×, so the backstop scales by N).
    let concurrency = config.eval_concurrency.unwrap_or(1).max(1);
    let transcripts_dir = match transcripts_dir(app) {
        Ok(d) => Some(d),
        Err(e) => {
            println!("[batch] WARN: no transcripts dir ({e}) — run proceeds without on-disk transcripts");
            None
        }
    };
    let sink: Arc<dyn BatchSink> = Arc::new(TauriBatchSink {
        app: app.clone(),
        collection_id: config.collection_id.clone(),
        transcripts_dir,
        backends: config.targets.iter().map(|t| (t.model.clone(), t.backend)).collect(),
    });
    let job_path = queue::run_path(&jobs_dir(app)?, &config.collection_id);
    let rec_path = job_path.clone();
    let record = move |u: &CompletedUnit| {
        let _ = queue::append(&rec_path, u); // durable save; best-effort vs the run
    };

    let turn_cancel = cancel.clone();
    let native_cancel = cancel.clone();
    let native_options = options.clone();
    let keep_alive = agentic_keep_alive(config.keep_alive);
    // The per-turn token budget is tier-scaled, so resolve the effective tier from the
    // (override-applied) TASKS — see `effective_tier`. NOT the UI `config.tier`, which is
    // often `Auto` (→ `None`) and would collapse every tier to Easy's cap.
    let tier = effective_tier(&tasks);

    // Native (tool-calling) pass FIRST — so a slow native run streams to the UI immediately
    // instead of waiting out the whole prompt pass. It fills `agentic_native_fc` on a column
    // skeleton; the prompt pass runs next and we merge the native aggregates into its report.
    let native_aggs: HashMap<String, AggAgentic> = if config.native {
        // Native FC follows the running server: probe each target on ITS backend
        // (Ollama via /api/show tools; llama.cpp with --jinja is tool-capable; MLX
        // has no native tool API). A batch is single-backend (UI), so this just
        // selects whichever server the user is on.
        let mut supported = HashSet::new();
        for t in &config.targets {
            if probe_native_tools(t.backend, &endpoint_for(t.backend), &t.model).await {
                supported.insert(t.model.clone());
            }
        }
        // Guard: native tool-calling is selected but NO target can run it — Ollama's /api/show
        // lists no `tools` capability (a custom-imported quant), MLX has no native tool API, or
        // the probe timed out. If native is the ONLY method, the run would otherwise skip every
        // model silently and return an all-null report (n=0). Refuse with an actionable message
        // (mirrors the Context-Stress-Test guard) instead of a silent no-result run. This Err
        // surfaces beside the RUN BATCH button via the batch-store error banner.
        if supported.is_empty() && !config.prompt {
            let names = config.targets.iter().map(|t| t.model.as_str()).collect::<Vec<_>>().join(", ");
            return Err(AppError::Inference(format!(
                "Native tool-calling isn't available for {names} on this backend — tick \
                 \"Prompt-based\" (under the model), or pick a model whose template advertises tool \
                 support, then re-run. (If the model IS tool-capable, it may have been busy loading \
                 when probed — try again.)"
            )));
        }
        let mut skeleton = skeleton_report(&config.collection_id, &config.targets);
        let targets = config.targets.clone();
        run_native_fc_pass(
            &mut skeleton,
            &tasks,
            &supported,
            native_cancel,
            |model, task| {
                let backend = targets.iter().find(|t| t.model == model).map(|t| t.backend).unwrap_or_default();
                // Gate the native system's answer-delivery mandate on act-vs-abstain, exactly
                // like the prompt path (runner.rs) — so a native model on an ACT task is told to
                // call the reporter tool, not nudged into prose (an unfair ReportedInProse).
                let terminal = match task.agentic.as_ref().map(|s| &s.end_state) {
                    Some(EndStateRule::RequireAll(_)) | Some(EndStateRule::RequireSequence(_)) => {
                        TerminalGuidance::MustUseTools
                    }
                    _ => TerminalGuidance::PlainTextOk,
                };
                // Native turns emit STRUCTURED tool_calls — often several parallel calls per turn
                // (plus any reasoning) — so the prompt path's terse 256-token cap truncates them
                // (→ EmptyOutput on hard/extreme). Give native the GENEROUS tier budget
                // (`max_tokens_for(tier, true)` = 1536–4096): tier-scaled, ample headroom, still
                // bounded (anti-runaway) and capped by the per-turn wall-clock timeout.
                let is_thinking = targets.iter().find(|t| t.model == model).is_some_and(|t| t.is_thinking);
                NativeToolTurn {
                    backend,
                    endpoint: endpoint_for(backend),
                    model: model.to_string(),
                    tools: task.tools.clone(),
                    options: native_options.clone(),
                    terminal,
                    max_tokens: max_tokens_for_preset(tier, true, config.think_preset),
                    is_thinking,
                }
            },
            prior,
            &record,
            &OllamaVramGate,
            sink.clone(),
            concurrency,
        )
        .await?; // a gate Err halts; per-task run errors are swallowed inside
        // Surface the native column right away — but ONLY as an INTERMEDIATE complete when the
        // prompt pass will follow (so the UI keeps "running"). If native is the only selected
        // pass, the skeleton becomes the final report below, emitted once.
        if config.prompt {
            log_emit(app, EVENT_BATCH_COMPLETE, BatchCompletePayload { report: skeleton.clone(), r#final: false });
        }
        skeleton.columns.into_iter().filter_map(|c| c.agentic_native_fc.map(|a| (c.model, a))).collect()
    } else {
        HashMap::new()
    };

    // Where Ollama placed each model's weights: a model spilled onto the CPU (didn't fit in VRAM)
    // runs several times slower, so the runner must grant it a larger per-step timeout (else a
    // progressing turn is killed as a false `TurnTimeout`). Probed once per target up front (the
    // per-turn closure is sync). llama.cpp/MLX report nothing here → not offloaded. The UI reads
    // the same placement via `ollama_model_placement` to show the "running on CPU" notice.
    let placements = run_facts::probe_placements(&config.targets, endpoint_for).await;
    // The per-turn closure needs only the bool (larger step timeout for a spilled model);
    // the full placement (weights/offload bytes + claimed quant) is stamped on the report.
    let cpu_offload: HashMap<String, bool> = placements.iter().map(|(m, p)| (m.clone(), p.on_cpu)).collect();

    // The hardware-adaptive `num_ctx` ceiling for THIS machine (bigger box → bigger window that
    // can hold a reasoning model's fixed per-turn budget + transcript). This is the ONLY knob
    // hardware moves; the budget itself (`max_tokens_for`) is a machine-independent constant so the
    // tier stays reproducible. Per target: Ollama honors per-request `num_ctx` so it gets the full
    // class band; llama.cpp FIXES its window at launch and ignores per-request `num_ctx`, so the
    // eval must clamp to the ACTUAL launched `-c` (which `plan_launch` may have RAM-clamped below
    // the band) — never promise budget the runtime can't hold. `/props` unreachable → band fallback.
    let band = agentic_ctx_ceiling(snapshot().total_memory_bytes);
    let mut ctx_ceilings: HashMap<String, u32> = HashMap::new();
    for t in &config.targets {
        let ceiling = match t.backend {
            BackendKind::LlamaCpp => probe_llama_n_ctx(&endpoint_for(t.backend)).await.map_or(band, |n| n.min(band)),
            _ => band,
        };
        ctx_ceilings.insert(t.model.clone(), ceiling);
    }
    let think_preset = config.think_preset; // captured by the sync per-turn closure below
    // The per-turn closure below MOVES the ceilings map; keep a copy to stamp onto the report
    // columns after the run (the closure only reads via `.get()`, so a clone is faithful).
    // `cpu_offloaded` needs no copy: `run_facts::stamp_placements` stamps it from the SAME
    // placement probe the closure's bool map was derived from.
    let ctx_ceilings_stamp = ctx_ceilings.clone();
    // The launched llama-server's facts — known only for a server WE spawned; an
    // externally-started one stamps `None` (its flags are unknowable, never guessed).
    // `(gguf stem, on-disk model bytes)` so the stamp below can require the running
    // server to actually be serving the column's model before claiming anything.
    let llama_state = app.state::<LlamaServerState>();
    let llama_kv_type = llama_state.kv_cache_type();
    let llama_server_model: Option<(String, Option<u64>)> = llama_state.running_summary().map(|(path, _)| {
        let stem = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        (stem, llama_state.readout().and_then(|r| r.model_bytes))
    });

    // Prompt pass — only when selected. When it's NOT, the report is the column skeleton that the
    // native aggregates merge into (a native-only run). At least one pass is guaranteed by the UI.
    let mut report = if config.prompt {
        run_batch_resumable(
            &config.collection_id,
            &config.targets,
            &tasks,
            cancel,
            sink.clone(),
            move |t: &ModelTarget| BackendTurn {
                backend: t.backend,
                endpoint: endpoint_for(t.backend),
                model: t.model.clone(),
                cancel: turn_cancel.clone(),
                options: options.clone(),
                keep_alive,
                is_thinking: t.is_thinking,
                max_tokens: max_tokens_for_preset(tier, t.is_thinking, think_preset),
                cpu_offloaded: cpu_offload.get(&t.model).copied().unwrap_or(false),
                ctx_ceiling: ctx_ceilings.get(&t.model).copied().unwrap_or(band),
                stop_cache: Default::default(),
            },
            prior,
            &record,
            &OllamaVramGate,
            concurrency,
        )
        .await?
    } else {
        skeleton_report(&config.collection_id, &config.targets)
    };
    // Merge the native aggregates (collected before the prompt pass) into its columns.
    // Placement facts stamp via the SHARED helper (`run_facts`) — the same one the qm CLI
    // uses, so the app's Latency view and `qm --costs` can never drift.
    run_facts::stamp_placements(&mut report.columns, &placements);
    for col in &mut report.columns {
        if let Some(a) = native_aggs.get(&col.model) {
            col.agentic_native_fc = Some(a.clone());
        }
        // Stamp the hardware-adaptive `num_ctx` ceiling this model ran under (surfaces on the
        // readiness verdict + publish payload so a slow/thinking run reads honestly).
        col.ctx_ceiling = ctx_ceilings_stamp.get(&col.model).copied();
        if col.backend == BackendKind::LlamaCpp {
            // Stamp launch facts ONLY when the running server serves THIS column's model
            // — never another model's bytes or flags. llama.cpp has no /api/ps:
            // `weights_total_bytes` here is the GGUF's on-disk size from the spawn
            // readout (no resident/VRAM split exists to report — labeled so).
            if let Some((stem, model_bytes)) = &llama_server_model {
                if llama_serves_model(stem, &col.model) {
                    col.kv_cache_type = llama_kv_type.clone();
                    col.weights_total_bytes = *model_bytes;
                }
            }
        }
    }
    report.num_ctx = config.params.as_ref().and_then(|p| p.num_ctx);
    // The FULL params this run sent, stamped like num_ctx — publish reads the report's params
    // (never the live global header, which may have been edited since the run).
    report.params = config.params.clone();
    // The batch-wide Thinking-Budget preset (reasoning scratchpad allowance) — carried to the report
    // so the verdict/report/publish can show "Ready @ Standard" and size `think_budget`.
    report.think_preset = Some(config.think_preset);
    // Fork-on-edit guard: stamp the content-verified hash from the RECEIVED tasks (pre-override).
    // `Some` only for a pristine bundled collection; `None` for custom OR any edit → unpublishable.
    report.collection_hash = verified_collection_hash(&config.collection_id, &config.tasks);
    // Stamp the running Ollama version so a native tool-calling regression on a version bump is
    // diagnosable (the honest garbled/foreign verdict reads as "at Ollama vX"). Best-effort.
    report.ollama_version = probe_ollama_version(&endpoint_for(BackendKind::Ollama)).await;
    log_emit(app, EVENT_BATCH_COMPLETE, BatchCompletePayload { report: report.clone(), r#final: true });

    if let Ok(dir) = history_dir(app) {
        let entries = batch_summaries(&report, &crate::time_iso::now_utc());
        if !entries.is_empty() {
            let _ = eval_history::append(&dir, &config.collection_id, &entries);
        }
    }

    // Transactional finish: persist → verify on disk → only THEN delete the log,
    // so a crash between save and delete can never lose the whole run.
    let reports_d = reports_dir(app)?;
    reports::save(&reports_d, &report)?;
    if reports::load(&reports_d, &config.collection_id)?.is_none() {
        return Err(AppError::Io("batch report did not persist — keeping the resumable job log".into()));
    }
    let _ = queue::delete(&job_path);
    Ok(report)
}

#[tauri::command]
pub fn stop_batch_eval(state: tauri::State<'_, BatchRunState>) -> Result<(), AppError> {
    if let Some(t) = state.cancel.lock_recover().take() {
        t.cancel();
    }
    Ok(())
}

// ── Crash-recovery: detect / resume / discard an interrupted run ──────────────

/// A leftover (interrupted) run the user can resume or discard.
#[derive(Serialize)]
pub struct UnfinishedRun {
    pub run_id: String,
    pub collection_id: String,
    pub done: usize,
    pub total: usize,
}

// Moved to the engine (`inference/eval/batch.rs`) so the headless CLI reaches it
// without this GUI command module; re-exported here for the existing GUI callers.
pub(crate) use crate::inference::eval::batch::probe_native_tools;

/// Upper bound on a run's units — prompt (targets × tasks) plus, when native is
/// on, the agentic tasks on each native-capable target. MLX has no native tool API
/// so it's excluded; an Ollama model without the `tools` capability is also a
/// no-native case, so this stays an upper bound (the actual native pass runs only
/// for probe-confirmed `supported` models).
fn total_units(c: &RunConfig) -> usize {
    let prompt = c.targets.len() * c.tasks.len();
    let native = if c.native {
        let native_capable = c.targets.iter().filter(|t| t.backend != BackendKind::Mlx).count();
        // The native pass runs every AGENTIC task — both "agentic" and "agent_loop" (see
        // `is_agentic`); counting only "agentic" under-sized the progress bar for agent_loop sets.
        let agentic = c.tasks.iter().filter(|t| crate::inference::eval::toolcall::tasks::is_agentic(&t.category)).count();
        native_capable * agentic
    } else {
        0
    };
    prompt + native
}

/// On app mount: is there an interrupted run to recover? Returns the first leftover
/// job log's collection + progress (a leftover `.jsonl` == an interrupted run).
#[tauri::command]
pub fn check_unfinished_run(app: AppHandle) -> Result<Option<UnfinishedRun>, AppError> {
    for path in queue::list_paths(&jobs_dir(&app)?)? {
        if let Some((config, units)) = queue::load(&path)? {
            return Ok(Some(UnfinishedRun {
                run_id: config.collection_id.clone(),
                collection_id: config.collection_id.clone(),
                done: units.len(),
                total: total_units(&config),
            }));
        }
    }
    Ok(None)
}

/// Resume an interrupted run: rebuild the completed units into ONE partial
/// `BatchReport`, emit it once (bulk rehydration — paints the Matrix instantly
/// without flooding the IPC bridge), then continue the live run, skipping the
/// completed units (prompt AND native).
#[tauri::command]
pub async fn resume_batch_eval(
    app: AppHandle,
    state: tauri::State<'_, BatchRunState>,
    run_id: String,
) -> Result<BatchReport, AppError> {
    let path = queue::run_path(&jobs_dir(&app)?, &run_id);
    let Some((config, units)) = queue::load(&path)? else {
        return Err(AppError::NotFound(format!("no interrupted run to resume for '{run_id}'")));
    };
    let partial = fold_report(&config.collection_id, &config.targets, &config.tasks, &units);
    // Partial replay before resuming — NOT final, run_passes emits the real final complete.
    log_emit(&app, EVENT_BATCH_COMPLETE, BatchCompletePayload { report: partial, r#final: false });
    run_passes(&app, &state, &config, &units).await
}

/// Throw away an interrupted run's log (Discard).
#[tauri::command]
pub fn discard_run(app: AppHandle, run_id: String) -> Result<(), AppError> {
    queue::delete(&queue::run_path(&jobs_dir(&app)?, &run_id))
}

/// Does the running llama-server (identified by its GGUF file STEM) serve this eval
/// column's model? Eval targets carry the FILE NAME (`gemma-4-12b-it_q4_k_m.gguf`) while
/// the stem drops the extension — comparing them raw silently unstamped every app-launched
/// llama run's model bytes ("Model in memory: N/A" under a live 4.4GB readout).
fn llama_serves_model(server_stem: &str, col_model: &str) -> bool {
    col_model == server_stem || col_model.strip_suffix(".gguf") == Some(server_stem)
}

#[cfg(test)]
mod llama_stamp_tests {
    use super::llama_serves_model;

    #[test]
    fn matches_with_and_without_the_gguf_suffix_and_never_across_models() {
        assert!(llama_serves_model("gemma-4-12b-it_q4_k_m", "gemma-4-12b-it_q4_k_m.gguf"));
        assert!(llama_serves_model("gemma-4-12b-it_q4_k_m", "gemma-4-12b-it_q4_k_m"));
        // A server serving a DIFFERENT model must never lend its bytes/flags.
        assert!(!llama_serves_model("qwen2.5-coder-7b-instruct_q4_k_m", "gemma-4-12b-it_q4_k_m.gguf"));
        // Suffix only strips at the end — no substring tricks.
        assert!(!llama_serves_model("gemma-4-12b", "gemma-4-12b-it_q4_k_m.gguf"));
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;
    use crate::inference::eval::agentic::sandbox::EndStateRule;
    use crate::inference::eval::agentic::spec::{AgenticSpec, DifficultyAxes};

    fn agentic(id: &str, k: Option<u32>, tier: Tier, axes: Option<DifficultyAxes>) -> ToolTask {
        ToolTask {
            id: id.into(),
            category: "agentic".into(),
            prompt: "p".into(),
            tools: vec![],
            expected: Default::default(),
            agentic: Some(AgenticSpec {
                mocks: vec![],
                mcp: None,
                end_state: EndStateRule::ExpectAbstainingText,
                environment: Default::default(),
                tier,
                axes,
                k,
                max_steps: None,
                faults: vec![],
                max_recovery: None,
                must_not_call: vec![],
                world_state: None,
                name_faults: vec![],
                generated: false,
                entity_tools: vec![],
                recognized_tools: vec![],
                safety: None,
                payload_noise: false,
                field_projections: Default::default(),
            }),
        }
    }

    fn single(id: &str) -> ToolTask {
        ToolTask {
            id: id.into(),
            category: "single".into(),
            prompt: "p".into(),
            tools: vec![],
            expected: Default::default(),
            agentic: None,
        }
    }

    fn spec(t: &ToolTask) -> &AgenticSpec {
        t.agentic.as_ref().unwrap()
    }

    #[test]
    fn tier_sets_tier_and_derives_locked_k_overriding_authored_k() {
        // Authored k=3 must yield to the tier-derived k so the run matches the locked
        // display (authored per-task k no longer silently wins under a chosen tier).
        let tasks = apply_overrides(vec![agentic("a", Some(3), Tier::Easy, None)], None, None, Some(Tier::Hard), None);
        let s = spec(&tasks[0]);
        assert_eq!(s.tier, Tier::Hard);
        assert_eq!(s.k, Some(pass_k_for(Tier::Hard))); // 16
    }

    #[test]
    fn explicit_k_wins_over_the_tier_derived_value() {
        // Custom escape hatch: an explicit UI k beats the tier policy.
        let tasks = apply_overrides(vec![agentic("a", None, Tier::Easy, None)], Some(7), None, Some(Tier::Extreme), None);
        assert_eq!(spec(&tasks[0]).k, Some(7));
    }

    #[test]
    fn decoy_tools_sets_axes_creating_default_axes_when_absent() {
        let tasks = apply_overrides(vec![agentic("a", None, Tier::Easy, None)], None, None, None, Some(4));
        assert_eq!(spec(&tasks[0]).axes.as_ref().unwrap().decoy_tools, 4);
    }

    #[test]
    fn no_overrides_leaves_the_authored_spec_intact() {
        let axes = DifficultyAxes { decoy_tools: 2, ..Default::default() };
        let tasks = apply_overrides(vec![agentic("a", Some(9), Tier::Medium, Some(axes))], None, None, None, None);
        let s = spec(&tasks[0]);
        assert_eq!(s.tier, Tier::Medium);
        assert_eq!(s.k, Some(9));
        assert_eq!(s.axes.as_ref().unwrap().decoy_tools, 2);
    }

    #[test]
    fn non_agentic_tasks_are_untouched() {
        let tasks = apply_overrides(vec![single("s")], Some(5), Some(8), Some(Tier::Hard), Some(3));
        assert!(tasks[0].agentic.is_none());
    }

    #[test]
    fn effective_tier_reads_the_tasks_real_tier_even_when_the_ui_tier_is_auto() {
        use crate::inference::eval::agentic::difficulty::passk::max_tokens_for;
        // The bug: a bundled Hard collection run with the tier dropdown on Auto (→ None) used
        // to budget every task at Easy's cap. With no UI override, each task keeps its
        // authored tier, and the effective tier must be that — NOT Easy.
        let tasks = apply_overrides(vec![agentic("a", None, Tier::Hard, None)], None, None, None, None);
        assert_eq!(effective_tier(&tasks), Tier::Hard);
        // And it composes to the right thinking budget (the symptom the user saw): the Hard
        // answer floor (2560) plus the FIXED reasoning scratchpad (see `think_tokens_for`).
        assert_eq!(max_tokens_for(effective_tier(&tasks), true), 2560 + 10240);
    }

    #[test]
    fn effective_tier_takes_the_hardest_tier_in_a_mixed_collection() {
        let tasks = vec![
            agentic("a", None, Tier::Easy, None),
            agentic("b", None, Tier::Extreme, None),
            single("s"),
        ];
        assert_eq!(effective_tier(&tasks), Tier::Extreme);
    }

    #[test]
    fn effective_tier_is_easy_when_no_task_is_agentic() {
        assert_eq!(effective_tier(&[single("s")]), Tier::Easy);
    }

    #[test]
    fn keep_alive_floors_when_unset_and_honors_an_explicit_override() {
        // Unset → the resident floor pins the model across the batch.
        assert_eq!(agentic_keep_alive(None), Some(AGENTIC_KEEP_ALIVE_SECS));
        // Explicit values win — forever, or a deliberately shorter window.
        assert_eq!(agentic_keep_alive(Some(-1)), Some(-1));
        assert_eq!(agentic_keep_alive(Some(30)), Some(30));
    }
}

#[cfg(test)]
mod fork_on_edit_tests {
    use super::*;
    use crate::inference::eval::agentic::sandbox::EndStateRule;

    fn pristine(id: &str) -> Vec<ToolTask> {
        load_v2_collection(v2_json(id).unwrap()).unwrap()
    }

    #[test]
    fn pristine_bundled_run_carries_the_real_hash() {
        let tasks = pristine("easy-webui-tasks");
        assert!(verified_collection_hash("easy-webui-tasks", &tasks).is_some());
        assert_eq!(verified_collection_hash("easy-webui-tasks", &tasks), collection_hash("easy-webui-tasks"));
    }

    #[test]
    fn custom_or_unknown_id_is_none() {
        let tasks = pristine("easy-webui-tasks");
        assert_eq!(verified_collection_hash("my-imported-collection", &tasks), None);
    }

    #[test]
    fn editing_world_state_forks_to_none() {
        let mut tasks = pristine("easy-webui-tasks");
        if let Some(spec) = tasks[0].agentic.as_mut() {
            spec.world_state = Some(serde_json::json!({ "route": "/hacked", "submitted": true }));
        }
        assert_eq!(verified_collection_hash("easy-webui-tasks", &tasks), None);
    }

    #[test]
    fn near_miss_single_char_edit_forks_to_none() {
        // ZERO TOLERANCE: a one-character change to a SINGLE field must sever the hash — proves the
        // Value compare has no normalization an attacker could exploit to publish a doctored
        // answer key as pristine.
        let mut a = pristine("easy-webui-tasks");
        a[0].prompt.push('!'); // one char, one field
        assert_eq!(verified_collection_hash("easy-webui-tasks", &a), None);

        // The same, buried in a RequireEndState target value (the answer key).
        let mut b = pristine("easy-webui-tasks");
        if let Some(spec) = b[0].agentic.as_mut() {
            if let EndStateRule::RequireEndState(target) = &mut spec.end_state {
                *target = serde_json::json!({ "fields": { "coupon": "SAVE11" }, "submitted": true });
            }
        }
        assert_eq!(verified_collection_hash("easy-webui-tasks", &b), None);
    }
}

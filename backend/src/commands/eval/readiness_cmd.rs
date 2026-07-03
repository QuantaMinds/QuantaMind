use crate::commands::emit::log_emit;
use crate::commands::eval::toolcall_cmd::{endpoint_for, list_builtin_collections};
use crate::commands::models::model_inspect::fetch_dims;
use crate::commands::prompt::prompt_options::{to_generate_options, validate_params};
use crate::commands::storage::storage::fetch_installed_with_stats;
use crate::commands::system::hardware::snapshot;
use crate::commands::llama::llama_server_types::{LlamaProbeReadiness, LlamaServerState};
use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::commands::eval::batch_cmd::probe_native_tools;
use crate::inference::eval::agentic::difficulty::passk::answer_tokens_for;
use crate::inference::eval::agentic::model_turn::{BackendTurn, NativeToolTurn};
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::toolcall::prompt::TerminalGuidance;
use crate::inference::eval::readiness::hardware::hwclass::{classify_bytes, default_required_tier, HardwareClass};
use crate::inference::eval::cliff::{build_ladder, run_cliff_with, run_cliff_with_factory, CliffPoint, CliffReport, CliffSource, StepProgress, DEFAULT_DEPTHS};
use crate::inference::eval::agentic::v2::scenarios::{v2_header, v2_json};
use crate::inference::eval::batch::BatchReport;
use crate::inference::eval::readiness::inputs::{resolve_quant, verdicts_for_column};
use crate::inference::eval::readiness::recommend;
use crate::inference::eval::readiness::profile::ReadinessProfile;
use crate::inference::eval::readiness::types::{CliffStatus, ModelVerdict};
use crate::inference::eval::readiness::vram_fit::{try_profile, Dims, MemoryProfile};
use crate::inference::eval::toolcall::tasks::{validate_tasks, ToolTask};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::persistence::prompts::schema::InferenceParams;
use crate::persistence::readiness::{cliff, profiles, reports};
use crate::sync::MutexExt;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

/// Live per-rung progress for the context-cliff probe (the panel's progress bar).
pub const EVENT_CLIFF_PROGRESS: &str = "cliff-progress";

/// Fine-grained sub-rung progress (per task generation) so the panel shows movement
/// DURING a slow padded rung — without it the bar sits frozen between rungs and reads
/// as "stuck" exactly when the model is working hardest (the deep-context rungs).
pub const EVENT_CLIFF_STEP: &str = "cliff-step";

/// Run-level cancellation for the context-cliff probe (mirrors `BatchRunState`): the
/// running probe stores its token here so `stop_context_cliff` can cancel it, and a
/// fresh run supersedes a previous one. Without this, Stop only hid the result in the
/// UI while the backend kept calling the model through the entire ladder.
#[derive(Default)]
pub struct CliffRunState {
    cancel: Mutex<Option<CancellationToken>>,
}

#[derive(Serialize, Clone)]
struct CliffProgress {
    /// The frontend's run token, echoed so a superseded run's late events can't be
    /// folded into the new run's series (model alone doesn't distinguish two runs of
    /// the same model).
    run_id: u32,
    model: String,
    done: usize,
    total: usize,
    /// The rung that just finished — carries its verified depth + composite so the
    /// chart grows live, not only at the final report.
    point: CliffPoint,
}

/// One fine-grained step (a single task generation) within a rung — drives the panel's
/// "rung r/N · position p/3 · task t/M" line and the ETA, so a long deep rung shows
/// continuous progress instead of a frozen bar.
#[derive(Serialize, Clone)]
struct CliffStep {
    /// The frontend's run token (same filtering contract as `CliffProgress`).
    run_id: u32,
    model: String,
    rung: usize,
    total_rungs: usize,
    target_tokens: u32,
    position: usize,
    total_positions: usize,
    task: usize,
    total_tasks: usize,
}

/// Context window headroom over the deepest rung: the system prompt (tool schemas),
/// the injected needle, and the output budget all sit on top of the padding, so the
/// window must exceed the requested token depth or the backend truncates the padding.
const CLIFF_CTX_HEADROOM: u32 = 2048;

/// llama.cpp isn't running the probe's model (or any) — guide the user to launch it at
/// a window the probe needs, naming both so the fix is unambiguous. The probe never
/// relaunches the server itself (it's user-managed), so this is the honest hand-off.
fn start_with_model_msg(model: &str, needed_ctx: u32) -> String {
    format!(
        "Start llama.cpp with \"{model}\" and a Context window of at least {needed_ctx} \
         tokens (set it in the parameters, then press Start) before running the Context \
         Stress Test. llama.cpp pins its context at launch, so the model must be loaded \
         with a window this deep before the probe can measure it."
    )
}

/// The right model is loaded but its launch `-c` is too small for the requested depth.
/// One honest line covering both "set it higher" and "this machine's memory caps it".
fn raise_or_reduce_msg(running_ctx: u32, needed_ctx: u32) -> String {
    let safe_depth = running_ctx.saturating_sub(CLIFF_CTX_HEADROOM);
    format!(
        "llama.cpp is running this model with a {running_ctx}-token context window, but \
         this probe needs about {needed_ctx}. Raise \"Context window\" and restart \
         llama.cpp (Stop & Start) — if it won't go higher, this machine's memory caps it \
         there, so reduce the Context Stress Test length to about {safe_depth} tokens."
    )
}

/// Look up an installed model's metadata, tolerant of the `:latest` tag mismatch
/// between an eval target and the `/api/tags` listing. Used for both the real
/// weight size and the real quantization.
fn registry_get<'a, V>(map: &'a HashMap<String, V>, model: &str) -> Option<&'a V> {
    let base = model.strip_suffix(":latest").unwrap_or(model);
    map.get(model).or_else(|| map.get(base)).or_else(|| map.get(&format!("{base}:latest")))
}

/// Act-vs-abstain mandate for a native cliff turn, derived from the task's end-state exactly
/// like the batch native pass: an ACT task (RequireAll / RequireSequence) tells the model to use
/// the tool; anything else leaves prose acceptable. Keeps native path-fair with prompt.
fn cliff_terminal(task: &ToolTask) -> TerminalGuidance {
    match task.agentic.as_ref().map(|s| &s.end_state) {
        Some(EndStateRule::RequireAll(_)) | Some(EndStateRule::RequireSequence(_)) => TerminalGuidance::MustUseTools,
        _ => TerminalGuidance::PlainTextOk,
    }
}

/// Device memory ceiling for the cliff VRAM pre-flight: unified (Apple) → system RAM;
/// a discrete GPU → its VRAM; unknown → `None` (fit stays unmeasured, never a guess).
/// Takes primitives (not the whole `HardwareSnapshot`) so it's unit-tested directly.
fn device_cap_bytes(unified: bool, total_memory_bytes: u64, vram_total_bytes: Option<u64>) -> Option<u64> {
    if unified {
        Some(total_memory_bytes)
    } else {
        vram_total_bytes
    }
}

/// The requested depth won't fit this machine's memory for an Ollama model (Ollama sizes
/// context per request, so it would silently spill to CPU or OOM mid-ladder). Names the
/// real footprint vs the cap and an ESTIMATED safe Max-Tokens — KV grows ~linearly with
/// context, so scaling the remaining budget is a fair estimate (labelled "about"), never a
/// fabricated exact figure.
fn cliff_vram_msg(model: &str, needed_ctx: u32, p: &MemoryProfile) -> String {
    let gb = |b: u64| format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0));
    let kv_budget = p.cap_bytes.saturating_sub(p.weights_bytes);
    let safe_ctx = if p.kv_cache_bytes > 0 {
        ((needed_ctx as u64).saturating_mul(kv_budget) / p.kv_cache_bytes) as u32
    } else {
        0
    };
    let safe_tokens = safe_ctx.saturating_sub(CLIFF_CTX_HEADROOM);
    format!(
        "This machine's memory ({cap}) can't hold a {needed_ctx}-token context for \"{model}\" \
         — it needs about {total} ({weights} weights + {kv} KV cache). Reduce the Context Stress \
         Test Max Tokens to about {safe_tokens} tokens, or use a smaller model/quant.",
        cap = gb(p.cap_bytes),
        total = gb(p.total_bytes),
        weights = gb(p.weights_bytes),
        kv = gb(p.kv_cache_bytes),
    )
}

/// Editable readiness profiles live as flat JSON here (built-ins seeded on first list).
fn profiles_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("readiness"))
}

/// The last persisted batch report per collection (written by `run_batch_eval`).
fn reports_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("batch_reports"))
}

/// Measured context-cliff depths per (collection, model) — written by the probe.
pub(crate) fn cliff_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("cliff"))
}

/// The probe writes one model's cliff outcome for a collection (atomic). `broken` ⇒
/// fails at the baseline; else `depth` `Some` ⇒ collapsed at that depth, `None` ⇒ no
/// cliff (held through `tested`). (NotProbed is never written — it's the absence of a
/// record.)
#[tauri::command]
pub fn save_cliff_result(
    app: AppHandle,
    collection_id: String,
    model: String,
    depth: Option<u32>,
    tested: u32,
    broken: bool,
) -> Result<(), AppError> {
    let status = if broken {
        CliffStatus::Broken { tested }
    } else {
        match depth {
            Some(d) => CliffStatus::Collapsed { depth: d },
            None => CliffStatus::NoCliff { tested },
        }
    };
    cliff::save(&cliff_dir(&app)?, &collection_id, &model, status)
}

/// The full per-model cliff status for a collection — the Matrix hydrates every state
/// (collapse depth, no-cliff, broken) from this so they survive a reload, not just
/// collapse depths.
#[tauri::command]
pub fn get_cliff_results(app: AppHandle, collection_id: String) -> Result<HashMap<String, CliffStatus>, AppError> {
    cliff::load(&cliff_dir(&app)?, &collection_id)
}

/// Run the context-cliff probe in the backend engine: pad the tasks to a ladder of
/// VERIFIED token depths (`0..=max_tokens`), sweep the needle across mid-document
/// positions, and report where tool-call accuracy collapses. Greedy (temp 0) so the
/// diagnostic reproduces; `num_ctx` is forced large enough that the deepest rung
/// isn't truncated. The classified outcome is persisted (verbatim model key) so the
/// Matrix/verdict can read it later; the full report (per-rung points) is returned
/// for the live chart. Tasks are validated here — the trust boundary holds even
/// when the command is invoked directly.
#[tauri::command]
pub async fn run_context_cliff(
    app: AppHandle,
    state: tauri::State<'_, CliffRunState>,
    llama_state: tauri::State<'_, LlamaServerState>,
    run_id: u32,
    model: String,
    backend: Option<BackendKind>,
    collection_id: String,
    tasks: Vec<ToolTask>,
    source: CliffSource,
    max_tokens: u32,
    steps: u32,
    params: Option<InferenceParams>,
    model_path: Option<String>,
    run_native_fc: Option<bool>,
) -> Result<CliffReport, AppError> {
    validate_tasks(&tasks)?;
    let backend = backend.unwrap_or_default();
    let native = run_native_fc.unwrap_or(false);

    // Start from the global header params, then force greedy (temp 0) and a context
    // window that fits the deepest rung plus the system/needle/output overhead.
    let mut options = match &params {
        Some(p) => {
            validate_params(p)?;
            to_generate_options(p)
        }
        None => GenerateOptions::default(),
    };
    options.temperature = Some(0.0);
    let needed_ctx = max_tokens.saturating_add(CLIFF_CTX_HEADROOM);
    if options.num_ctx.map_or(true, |c| c < needed_ctx) {
        options.num_ctx = Some(needed_ctx);
    }

    // llama.cpp pins context at launch and the probe never relaunches (the server is
    // user-managed), so verify up front that the RIGHT model is loaded with a wide
    // enough `-c`. Without this the ladder would 400 on every deep rung — or worse,
    // silently score whatever other model is loaded (the request `model` field is
    // ignored by the single-model server). Ollama/MLX size per request, so skip them.
    if backend == BackendKind::LlamaCpp {
        let path = model_path.as_deref().unwrap_or("");
        match llama_state.probe_readiness(path) {
            LlamaProbeReadiness::NotRunning | LlamaProbeReadiness::WrongModel => {
                return Err(AppError::Inference(start_with_model_msg(&model, needed_ctx)));
            }
            LlamaProbeReadiness::Ready { ctx } if ctx < needed_ctx => {
                return Err(AppError::Inference(raise_or_reduce_msg(ctx, needed_ctx)));
            }
            LlamaProbeReadiness::Ready { .. } => {}
        }
    }

    // Ollama sizes `num_ctx` per REQUEST, so a too-deep ladder silently spills to CPU or
    // OOMs mid-run — a different failure from llama.cpp's spawn-time window (guarded above),
    // so a separate, additive guard. Pre-flight the DEEPEST rung's memory fit with the same
    // exact-weights + real-KV-vs-cap estimate the readiness table uses, and refuse up front
    // with a readable "reduce Max Tokens" message. MLX exposes no weights/dims to estimate,
    // so it's left to size per request as before. Only blocks when the fit is actually
    // MEASURABLE (weights + dims + a device cap all present) — a missing input is never a
    // guessed block.
    if backend == BackendKind::Ollama {
        let hw = snapshot();
        if let Some(cap) = device_cap_bytes(hw.gpu.unified, hw.total_memory_bytes, hw.gpu.vram_total_bytes) {
            let installed = fetch_installed_with_stats(endpoint::OLLAMA).await.unwrap_or_default();
            let weights: HashMap<String, u64> = installed.iter().map(|m| (m.name.clone(), m.size_bytes)).collect();
            let w = registry_get(&weights, &model).copied();
            let dims = match w {
                Some(_) => fetch_dims(&model).await.map(|d| Dims {
                    layers: d.layers,
                    head_count: d.head_count,
                    head_count_kv: d.head_count_kv,
                    embedding_length: d.embedding_length,
                    context_length: d.context_length as u32,
                    kv_estimated: d.kv_estimated,
                }),
                None => None,
            };
            if let Some(profile) = try_profile(w, dims, Some(needed_ctx), Some(cap)) {
                if !profile.fits {
                    return Err(AppError::Inference(cliff_vram_msg(&model, needed_ctx, &profile)));
                }
            }
        }
    }

    // Native tool-calling gate: MLX has no native tool API, and a model whose template lacks
    // tool support can't run native either — refuse up front (mirrors the batch's
    // `probe_native_tools`) so the user gets a clear "switch to Prompt-based" instead of a
    // ladder of empty tool calls.
    if native && !probe_native_tools(backend, &endpoint_for(backend), &model).await {
        return Err(AppError::Inference(format!(
            "\"{model}\" can't run native tool-calling on this backend — switch the Context Stress Test to Prompt-based (or start llama.cpp with --jinja / use an Ollama model with tool support)."
        )));
    }

    // Register this run's cancel token so `stop_context_cliff` can abort it, and
    // supersede any previous run (mirrors the batch dispatcher).
    let cancel = CancellationToken::new();
    {
        let mut g = state.cancel.lock_recover();
        if let Some(prev) = g.take() {
            prev.cancel();
        }
        *g = Some(cancel.clone());
    }

    let ladder = build_ladder(max_tokens, steps);
    // Shared progress events — bound once so the prompt and native runs emit identically.
    let mut on_rung = |done: usize, total: usize, point: &CliffPoint| {
        log_emit(&app, EVENT_CLIFF_PROGRESS, CliffProgress { run_id, model: model.clone(), done, total, point: point.clone() });
    };
    let mut on_step = |s: StepProgress| {
        log_emit(
            &app,
            EVENT_CLIFF_STEP,
            CliffStep {
                run_id,
                model: model.clone(),
                rung: s.rung,
                total_rungs: s.total_rungs,
                target_tokens: s.target_tokens,
                position: s.position,
                total_positions: s.total_positions,
                task: s.task,
                total_tasks: s.total_tasks,
            },
        );
    };

    // A Stop makes the run return Err, so `?` short-circuits BEFORE the persistence below — a
    // cancelled probe never overwrites the saved cliff status. The two methods differ ONLY in the
    // per-task turn: prompt reuses one BackendTurn (via the engine's blanket &M factory seam);
    // native builds a fresh NativeToolTurn carrying each task's tool schemas, whose structured
    // tool_calls are canonicalized to the same JSON the scorer already parses.
    let report = if native {
        let endpoint = endpoint_for(backend);
        let make_native = |task: &ToolTask| NativeToolTurn {
            backend,
            endpoint: endpoint.clone(),
            model: model.clone(),
            tools: task.tools.clone(),
            options: Some(options.clone()),
            // Gate the answer-delivery mandate on act-vs-abstain, like the batch native pass, so a
            // native model on an ACT task is told to call the tool, not nudged into prose.
            terminal: cliff_terminal(task),
            max_tokens: answer_tokens_for(Tier::Easy),
            is_thinking: false,
        };
        run_cliff_with_factory(&make_native, &model, &tasks, &source, &ladder, &DEFAULT_DEPTHS, &cancel, &mut on_rung, &mut on_step).await?
    } else {
        let turn = BackendTurn {
            backend,
            endpoint: endpoint_for(backend),
            model: model.clone(),
            cancel: cancel.clone(),
            options: Some(options),
            keep_alive: None,
            // Readiness is a minimal liveness probe, not a scored agentic run — keep the non-thinking
            // budget, but at the answer floor so the probe's own tool call can't truncate.
            is_thinking: false,
            max_tokens: answer_tokens_for(Tier::Easy),
            cpu_offloaded: false, // liveness probe, not a scored run — no need to grant extra time
            ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING, // probe: fixed fallback window
            stop_cache: Default::default(),
        };
        run_cliff_with(&turn, &model, &tasks, &source, &ladder, &DEFAULT_DEPTHS, &cancel, &mut on_rung, &mut on_step).await?
    };

    // Persist the classified outcome (NotProbed is the absence of a record). A NATIVE cliff is
    // saved under a method-namespaced key so it never clobbers the prompt-based cliff the readiness
    // verdict reads (per eval-metric-comparability); the bare model key stays prompt-based, which is
    // what old records and the verdict already assume.
    if !collection_id.is_empty() && report.status != CliffStatus::NotProbed {
        let key = if native { format!("{model}::native_fc") } else { model.clone() };
        let _ = cliff::save(&cliff_dir(&app)?, &collection_id, &key, report.status.clone());
    }
    Ok(report)
}

/// Cancel the in-flight context-cliff probe (the Stop Probe button). Cancelling the
/// shared token aborts the current generation and the rung loop, so the model stops
/// being called and the partial result is never persisted. A no-op when idle.
#[tauri::command]
pub fn stop_context_cliff(state: tauri::State<'_, CliffRunState>) -> Result<(), AppError> {
    if let Some(t) = state.cancel.lock_recover().take() {
        t.cancel();
    }
    Ok(())
}

/// Hardware → recommended difficulty tier, derived from total system memory. The
/// single source of truth for the eval page's tier-`Auto` mode and HW hint: the GB
/// thresholds + class→tier policy live in `hwclass.rs`, never duplicated in TS.
#[derive(Serialize)]
pub struct HardwareTier {
    pub total_memory_bytes: u64,
    pub class: String,
    pub recommended_tier: Tier,
}

/// Stable human label for a class — decoupled from the `Debug` derive so the IPC
/// contract can't shift if a variant is renamed.
fn class_label(c: HardwareClass) -> &'static str {
    match c {
        HardwareClass::Constrained => "Constrained",
        HardwareClass::Mainstream => "Mainstream",
        HardwareClass::Workstation => "Workstation",
        HardwareClass::Frontier => "Frontier",
    }
}

/// Classify the running machine and recommend the difficulty tier its class should
/// clear (the eval page's `Auto` tier + the "HW: …" hint read this). Reuses the
/// readiness engine's `classify_bytes` + `default_required_tier` so the eval-run
/// path and the readiness verdict agree on one set of thresholds.
#[tauri::command]
pub fn get_hardware_tier() -> Result<HardwareTier, AppError> {
    let bytes = snapshot().total_memory_bytes;
    let class = classify_bytes(bytes);
    Ok(HardwareTier {
        total_memory_bytes: bytes,
        class: class_label(class).to_string(),
        recommended_tier: default_required_tier(class),
    })
}

#[tauri::command]
pub fn list_readiness_profiles(app: AppHandle) -> Result<Vec<ReadinessProfile>, AppError> {
    profiles::list(&profiles_dir(&app)?)
}

#[tauri::command]
pub fn save_readiness_profile(app: AppHandle, profile: ReadinessProfile) -> Result<(), AppError> {
    profiles::save(&profiles_dir(&app)?, &profile)
}

#[tauri::command]
pub fn delete_readiness_profile(app: AppHandle, id: String) -> Result<(), AppError> {
    profiles::delete(&profiles_dir(&app)?, &id)
}

/// The built-in collection ids sharing a collection's domain — its tier siblings
/// (e.g. `easy-coding`/`medium-coding`/`hard-coding` for the "coding" domain). Empty
/// for a custom collection (absent from the built-in registry) or one with no declared
/// domain, so the per-domain tier merge is then a no-op (single-collection behaviour).
fn sibling_collection_ids(collection_id: &str) -> Vec<String> {
    let domain = match v2_json(collection_id).and_then(v2_header).map(|h| h.domain) {
        Some(d) if !d.is_empty() => d,
        _ => return Vec::new(),
    };
    list_builtin_collections().into_iter().filter(|c| c.domain == domain).map(|c| c.id).collect()
}

/// Assess the collection's last persisted batch report against a profile. Scoring
/// is `readiness::assess` — the one source of truth shared with the future CLI;
/// this command adds no scoring logic of its own. When `cap_bytes` is set it also
/// measures VRAM fit for each **Ollama** column (exact weights + real KV cache at
/// the run's `num_ctx` vs the cap); single-model backends and an absent cap leave
/// fit unmeasured (`memory = None`) — never a guessed fit. An empty vec means no
/// run has been persisted yet (the page shows an empty state).
#[tauri::command]
pub async fn assess_readiness(
    app: AppHandle,
    collection_id: String,
    profile_id: String,
    cap_bytes: Option<u64>,
) -> Result<Vec<ModelVerdict>, AppError> {
    let profile = profiles::load(&profiles_dir(&app)?, &profile_id)?;
    let report = match reports::load(&reports_dir(&app)?, &collection_id)? {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    // Real model metadata by name (Ollama `/api/tags` + `/api/show`): the weight
    // size (for VRAM fit) and the real quantization (for the table — never guessed).
    // Best-effort: if Ollama is down the maps are empty and those fields stay N/A
    // rather than failing the assess.
    let installed = fetch_installed_with_stats(endpoint::OLLAMA).await.unwrap_or_default();
    let weights: HashMap<String, u64> = installed.iter().map(|m| (m.name.clone(), m.size_bytes)).collect();
    let quants: HashMap<String, String> =
        installed.iter().filter(|m| !m.quantization.is_empty()).map(|m| (m.name.clone(), m.quantization.clone())).collect();

    // Measured context-cliff depths for this collection (verbatim model keys). The
    // verdict only blocks on these when a profile opts in via `min_context_tokens`.
    let cliffs = cliff::load(&cliff_dir(&app)?, &collection_id).unwrap_or_default();

    // Per-domain tier accumulation: built-in tiers are SEPARATE single-tier
    // collections (easy-coding, medium-coding, …), so a single report's `by_tier`
    // only ever holds one tier. Load the persisted reports of the same domain's other
    // tier collections so the Tier Progression Matrix shows the full ladder. A custom
    // collection (`v2_json` = None) has no domain siblings → the merge is a no-op.
    let rdir = reports_dir(&app)?;
    let sibling_reports: Vec<BatchReport> = sibling_collection_ids(&collection_id)
        .into_iter()
        .filter(|id| id != &collection_id)
        .filter_map(|id| reports::load(&rdir, &id).ok().flatten())
        .collect();
    let sibling_refs: Vec<&BatchReport> = sibling_reports.iter().collect();

    let mut out = Vec::with_capacity(report.columns.len());
    for col in &report.columns {
        let memory = if cap_bytes.is_some() && col.backend == BackendKind::Ollama {
            let w = registry_get(&weights, &col.model).copied();
            let dims = match w {
                Some(_) => fetch_dims(&col.model).await.map(|d| Dims {
                    layers: d.layers,
                    head_count: d.head_count,
                    head_count_kv: d.head_count_kv,
                    embedding_length: d.embedding_length,
                    context_length: d.context_length as u32,
                    kv_estimated: d.kv_estimated,
                }),
                None => None,
            };
            try_profile(w, dims, report.num_ctx, cap_bytes)
        } else {
            None
        };
        let fits_in_vram = memory.as_ref().map(|m| m.fits);
        let vram_pressure = memory.as_ref().map(|m| m.pressure).unwrap_or(false);
        let cliff = registry_get(&cliffs, &col.model).copied().unwrap_or_default();
        // Real quant: the Ollama registry first, else parsed from the model name (a
        // GGUF/llama.cpp/MLX or offline-Ollama model) so the row can publish. Model-level —
        // shared across the column's per-path rows.
        let quantization = resolve_quant(registry_get(&quants, &col.model).cloned(), &col.model);
        // One verdict PER MEASURED PATH (native + prompt): the per-path emission sources each
        // row's metrics + tier ladder strictly from its own pass, sharing the model-level
        // memory/cliff/quant. The tier ladder accumulates across this domain's siblings for
        // the SAME path (matched on (model, backend)); the selected collection's own entries win.
        out.extend(verdicts_for_column(
            col,
            fits_in_vram,
            vram_pressure,
            cliff,
            memory,
            quantization,
            &profile,
            &sibling_refs,
            report.think_preset.unwrap_or_default(),
        ));
    }
    // Phase 7.3: rank best-first (Ready > Conditional > NotReady, ties by effort
    // then steps) so the page's recommendation banner + leaderboard are correct.
    recommend::rank(&mut out);
    Ok(out)
}

#[cfg(test)]
mod cliff_preflight_tests {
    use super::*;

    /// The "wrong/no model" hand-off must name the model and the window so the user
    /// can act; the frontend `friendly()` mapping also keys off this phrasing.
    #[test]
    fn start_with_model_msg_names_the_model_and_window() {
        let m = start_with_model_msg("qwen2.5-coder", 18_432);
        assert!(m.contains("qwen2.5-coder"), "names the target model: {m}");
        assert!(m.contains("18432"), "names the needed window: {m}");
        assert!(m.contains("Start llama.cpp"), "tells the user to start it: {m}");
    }

    /// The "too small" message must state both levers (raise + restart, or reduce depth)
    /// and a concrete safe depth = running window minus the cliff headroom.
    #[test]
    fn raise_or_reduce_msg_states_both_levers_and_a_safe_depth() {
        let m = raise_or_reduce_msg(8192, 18_432);
        assert!(m.contains("8192"), "names the running window: {m}");
        assert!(m.contains("18432"), "names the needed window: {m}");
        assert!(m.contains("Context window"), "names the raise lever: {m}");
        assert!(m.contains("6144"), "a safe depth of 8192 - 2048 headroom: {m}");
    }

    /// The device cap mirrors `deviceMemory`: unified → system RAM; discrete → its VRAM;
    /// an unknown discrete GPU (no VRAM readout) → None so the fit stays unmeasured.
    #[test]
    fn device_cap_bytes_picks_unified_ram_or_discrete_vram() {
        assert_eq!(device_cap_bytes(true, 32_000, Some(24_000)), Some(32_000)); // Apple: system RAM
        assert_eq!(device_cap_bytes(false, 32_000, Some(24_000)), Some(24_000)); // discrete: VRAM
        assert_eq!(device_cap_bytes(false, 32_000, None), None); // unknown → unmeasured, never a guess
    }

    /// The VRAM-won't-fit message names the machine cap, the footprint, and an estimated
    /// safe Max-Tokens (KV scales ~linearly with context, so the remaining budget scales it).
    #[test]
    fn cliff_vram_msg_names_footprint_and_a_safe_depth() {
        // cap 10 GB, weights 6 GB, KV 8 GB at needed_ctx 16384 → total 14 GB > 10 GB (won't fit).
        // kv budget = 10 - 6 = 4 GB; safe_ctx ≈ 16384 * 4/8 = 8192; safe_tokens = 8192 - 2048 = 6144.
        let gb = 1024u64 * 1024 * 1024;
        let p = MemoryProfile {
            weights_bytes: 6 * gb,
            kv_cache_bytes: 8 * gb,
            total_bytes: 14 * gb,
            cap_bytes: 10 * gb,
            context_length: 16_384,
            fits: false,
            pressure: false,
            estimated: false,
        };
        let m = cliff_vram_msg("gemma-3-12b", 16_384, &p);
        assert!(m.contains("gemma-3-12b"), "names the model: {m}");
        assert!(m.contains("16384"), "names the needed context: {m}");
        assert!(m.contains("6144"), "an estimated safe depth: {m}");
        assert!(m.to_lowercase().contains("reduce"), "tells the user to reduce Max Tokens: {m}");
    }

    /// The native-method gate refuses MLX outright (it has no native tool-calling API) without a
    /// network call — so a native cliff on MLX fails fast with a clear "switch to Prompt-based".
    #[tokio::test]
    async fn native_tools_gate_refuses_mlx() {
        assert!(!probe_native_tools(BackendKind::Mlx, "", "any-model").await);
    }
}

#[cfg(test)]
mod hardware_tier_tests {
    use super::*;

    #[test]
    fn reports_a_real_machine_and_a_consistent_recommended_tier() {
        let ht = get_hardware_tier().expect("hardware tier");
        // Real bytes, not a guessed fallback.
        assert!(ht.total_memory_bytes > 0);
        // Class label is one of the four engine classes.
        assert!(["Constrained", "Mainstream", "Workstation", "Frontier"].contains(&ht.class.as_str()));
        // The command adds no policy of its own — it must agree with the engine.
        let class = classify_bytes(ht.total_memory_bytes);
        assert_eq!(ht.class, class_label(class));
        assert_eq!(ht.recommended_tier, default_required_tier(class));
    }

    #[test]
    fn serializes_with_the_ipc_contract_keys_and_snake_case_tier() {
        let ht = HardwareTier {
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
            class: "Mainstream".into(),
            recommended_tier: Tier::Medium,
        };
        let v = serde_json::to_value(&ht).unwrap();
        assert_eq!(v["total_memory_bytes"], 16u64 * 1024 * 1024 * 1024);
        assert_eq!(v["class"], "Mainstream");
        assert_eq!(v["recommended_tier"], "medium"); // matches TS TierSchema enum
    }
}

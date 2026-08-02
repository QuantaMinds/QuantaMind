use crate::commands::emit::log_emit;
use crate::commands::eval::toolcall_cmd::{endpoint_for, list_builtin_collections};
use crate::commands::models::model_inspect::fetch_dims;
use crate::commands::prompt::prompt_options::{to_generate_options, validate_params};
use crate::commands::storage::storage_types::InstalledModelInfo;
use crate::commands::system::hardware::snapshot;
use crate::commands::llama::llama_server_types::{LlamaProbeReadiness, LlamaServerState};
use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::vram_math::KvPrecision;
use crate::commands::eval::batch_cmd::probe_native_tools;
use crate::inference::eval::agentic::difficulty::passk::{answer_tokens_for, ThinkPreset};
use crate::inference::eval::agentic::model_turn::{BackendTurn, NativeToolTurn};
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::toolcall::prompt::TerminalGuidance;
use crate::inference::eval::readiness::hardware::hwclass::{classify_bytes, default_required_tier, HardwareClass};
use crate::inference::eval::cliff::{build_ladder, run_cliff_with, run_cliff_with_factory, CliffBudget, CliffPoint, CliffReport, CliffSource, StepProgress, CLIFF_BASE_HEADROOM, DEFAULT_DEPTHS};
use crate::inference::eval::agentic::v2::scenarios::{v2_header, v2_json};
use crate::inference::eval::batch::BatchReport;
use crate::inference::eval::readiness::inputs::{resolve_quant, verdicts_for_column};
use crate::inference::eval::readiness::recommend;
use crate::inference::eval::readiness::profile::ReadinessProfile;
use crate::inference::eval::readiness::types::{CliffStatus, ModelVerdict};
use crate::commands::llama::llama_runtime::{plan_launch, KvDims};
use crate::commands::storage::storage_disk;
use crate::inference::eval::readiness::rightsizing::right_size::{self, RightSizingGroup};
use crate::inference::eval::readiness::vram_fit::{self, try_profile, Dims, MemoryProfile};
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

/// Every model installed in the shared weights folder, read straight off disk.
/// Replaces the old model-registry API call: the GGUF header carries the same
/// facts the readiness table needs (weight bytes for the VRAM fit, the real
/// quantization for the table). Best-effort — an unreadable folder yields an
/// empty list and those fields stay N/A rather than failing the assess.
fn installed_models() -> Vec<InstalledModelInfo> {
    let dir = crate::commands::storage::storage_disk::gguf_dir();
    crate::commands::llama::llama_discover::discover_gguf_models(&[dir.as_path()])
}

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
/// This is the NON-THINKING base (canonical in `cliff::budget`); a thinking run's real
/// headroom is `CliffBudget::headroom(max_tokens)`, which adds the deepest rung's
/// scratchpad — the gates below take that computed value, never this const.
const CLIFF_CTX_HEADROOM: u32 = CLIFF_BASE_HEADROOM;

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
/// `headroom` is the RUN's computed reserve (base + any thinking scratchpad), so the
/// suggested safe depth stays true when a thinking budget widens the reserve.
fn raise_or_reduce_msg(running_ctx: u32, needed_ctx: u32, headroom: u32) -> String {
    let safe_depth = running_ctx.saturating_sub(headroom);
    format!(
        "llama.cpp is running this model with a {running_ctx}-token context window, but \
         this probe needs about {needed_ctx}. Raise \"Context window\" and restart \
         llama.cpp (Stop & Start) — if it won't go higher, this machine's memory caps it \
         there, so reduce the Context Stress Test length to about {safe_depth} tokens."
    )
}

/// The readiness error (if any) for a llama.cpp cliff probe. Pure over the probe result so the
/// gate unit-tests without a running server. An EMPTY `path` is a CALLER bug (the re-probe
/// dropped the GGUF path): it can never equal the running server's real path, so without this it
/// would masquerade as `WrongModel` and emit the misleading "start with a bigger context"
/// message. Fail honestly and distinctly instead.
fn llama_cliff_gate(path: &str, readiness: LlamaProbeReadiness, model: &str, needed_ctx: u32, headroom: u32) -> Option<AppError> {
    if path.is_empty() {
        return Some(AppError::Inference(format!(
            "No model path was provided for the llama.cpp Context Stress Test of \"{model}\" — \
             reselect the model in the picker, then run."
        )));
    }
    match readiness {
        LlamaProbeReadiness::NotRunning | LlamaProbeReadiness::WrongModel => {
            Some(AppError::Inference(start_with_model_msg(model, needed_ctx)))
        }
        LlamaProbeReadiness::Ready { ctx } if ctx < needed_ctx => {
            Some(AppError::Inference(raise_or_reduce_msg(ctx, needed_ctx, headroom)))
        }
        LlamaProbeReadiness::Ready { .. } => None,
    }
}

/// The requested depth doesn't fit the model's own context window. Names the deepest
/// Max Tokens that CAN be measured, so the fix is a concrete number, not a direction.
/// `headroom` is the run's computed reserve — under a thinking budget it includes the
/// scratchpad, so the suggested number stays achievable, never optimistic.
fn cliff_window_msg(model: &str, context_length: u32, needed_ctx: u32, headroom: u32) -> String {
    let usable = context_length.saturating_sub(headroom);
    format!(
        "This probe needs about {needed_ctx} tokens of context, but \"{model}\" only has a \
         {context_length}-token window. The tool schemas, the injected task, and the reply \
         (including any thinking budget) all sit on top of the padding, so Max Tokens must stay \
         about {headroom} below the window. Reduce the Context Stress Test Max Tokens to \
         {usable} or less."
    )
}

/// The model's own context window is a HARD ceiling on a measurable depth, and exceeding it
/// fails SILENTLY rather than loudly: the server clamps `num_ctx` down to the trained window and
/// truncates the prompt — which deletes the injected needle and pins `prompt_eval_count` at
/// the window, so the rung fails for a reason the model never caused and its "verified" depth
/// is a saturated counter (a fabricated number). Refuse up front instead. Pure over the
/// probed window so it unit-tests without a server; `None` window ⇒ unmeasurable ⇒ never a
/// guessed block (same rule as the VRAM gate).
fn cliff_window_gate(context_length: Option<u32>, model: &str, needed_ctx: u32, headroom: u32) -> Option<AppError> {
    match context_length {
        Some(ctx) if needed_ctx > ctx => Some(AppError::Inference(cliff_window_msg(model, ctx, needed_ctx, headroom))),
        _ => None,
    }
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

/// VRAM fit for a llama.cpp column, graded at the KV precision the launch would
/// ACTUALLY use on this machine — `plan_launch` downgrades to a Q8 cache under memory
/// pressure, and the resulting profile says so (`kv_precision`), which `assess` turns
/// into an explicit advisory condition. Pure over GGUF metadata so it unit-tests
/// without files; any missing dimension → `None` (unmeasured, never guessed).
fn llama_profile_from_meta(
    weights_bytes: u64,
    meta: &crate::inference::gguf::gguf::GgufMetadata,
    num_ctx: Option<u32>,
    cap_bytes: Option<u64>,
    total_memory_bytes: u64,
) -> Option<MemoryProfile> {
    let (layers, head_count, embedding_length) = (meta.block_count?, meta.head_count?, meta.embedding_length?);
    let head_count_kv = meta.head_count_kv.unwrap_or(head_count);
    let kv_dims = KvDims { layers, head_count, head_count_kv, embedding_length };
    let plan = plan_launch(Some(weights_bytes), Some(kv_dims), total_memory_bytes, meta.context_length, num_ctx);
    let dims = Dims {
        layers,
        head_count,
        head_count_kv,
        embedding_length,
        context_length: meta.context_length.unwrap_or(vram_fit::DEFAULT_FALLBACK_CTX),
        kv_estimated: meta.head_count_kv.is_none(),
    };
    try_profile(Some(weights_bytes), Some(dims), num_ctx, cap_bytes, plan.kv.precision())
}

/// The requested depth won't fit this machine's memory for a model (the server sizes
/// context per request, so it would silently spill to CPU or OOM mid-ladder). Names the
/// real footprint vs the cap and an ESTIMATED safe Max-Tokens — KV grows ~linearly with
/// context, so scaling the remaining budget is a fair estimate (labelled "about"), never a
/// fabricated exact figure.
fn cliff_vram_msg(model: &str, needed_ctx: u32, p: &MemoryProfile, headroom: u32) -> String {
    let gb = |b: u64| format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0));
    let kv_budget = p.cap_bytes.saturating_sub(p.weights_bytes);
    let safe_ctx = if p.kv_cache_bytes > 0 {
        ((needed_ctx as u64).saturating_mul(kv_budget) / p.kv_cache_bytes) as u32
    } else {
        0
    };
    let safe_tokens = safe_ctx.saturating_sub(headroom);
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
            Some(d) => CliffStatus::Collapsed { depth: d, concentration: None },
            None => CliffStatus::NoCliff { tested, saturated: false }, // primitive path — cannot claim zero failures
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
    is_thinking: Option<bool>,
    think_preset: Option<ThinkPreset>,
) -> Result<CliffReport, AppError> {
    validate_tasks(&tasks)?;
    let backend = backend.unwrap_or_default();
    let native = run_native_fc.unwrap_or(false);
    // The probe's output budget: answer floor for every model; a thinking model adds a
    // scratchpad banded to each rung's depth (mirrors the Tests page's tier presets).
    // Absent args ⇒ the non-thinking default — byte-identical to the pre-preset probe.
    let budget = CliffBudget {
        is_thinking: is_thinking.unwrap_or(false),
        preset: think_preset.unwrap_or_default(),
        // Flat-cap is a CLI-only experimental control; the GUI always runs banded.
        flat_cap: None,
    };

    // Start from the global header params, then force greedy (temp 0) and a context
    // window that fits the deepest rung plus the system/needle/output overhead —
    // where "output" includes the deepest rung's thinking scratchpad when one is on.
    let mut options = match &params {
        Some(p) => {
            validate_params(p)?;
            to_generate_options(p)
        }
        None => GenerateOptions::default(),
    };
    // The probe starts from the user's GLOBAL params: a set temperature is honored
    // (production parity — measure the config you deploy at; Miller's stated exception:
    // studying the model AT a temperature is a legitimate purpose). Greedy 0 is the
    // DEFAULT when unset, not a pin — same rule the CLI has always applied. The
    // effective temperature is stamped on the report so a sampled depth is never
    // conflated with a greedy one (metric comparability).
    if options.temperature.is_none() {
        options.temperature = Some(0.0);
    }
    let effective_temperature = options.temperature;
    let headroom = budget.headroom(max_tokens);
    let needed_ctx = max_tokens.saturating_add(headroom);
    if options.num_ctx.map_or(true, |c| c < needed_ctx) {
        options.num_ctx = Some(needed_ctx);
    }

    // llama.cpp pins context at launch and the probe never relaunches (the server is
    // user-managed), so verify up front that the RIGHT model is loaded with a wide
    // enough `-c`. Without this the ladder would 400 on every deep rung — or worse,
    // silently score whatever other model is loaded (the request `model` field is
    // ignored by the single-model server). some backends size per request, so skip them.
    if backend == BackendKind::LlamaCpp {
        let path = model_path.as_deref().unwrap_or("");
        if let Some(err) = llama_cliff_gate(path, llama_state.probe_readiness(path), &model, needed_ctx, headroom) {
            return Err(err);
        }
    }

    // Additive pre-flight: even with a wide enough server window, the DEEPEST rung's
    // KV cache plus the weights may not fit this machine — which shows up as a CPU
    // spill or a mid-run OOM rather than a clean error. Estimate it up front from the
    // exact on-disk weight bytes and the GGUF's real dims (the same inputs the
    // readiness table uses) and refuse with a readable "reduce Max Tokens". Only
    // blocks when the fit is actually MEASURABLE (weights + dims + a device cap all
    // present) — a missing input is never a guessed block.
    if backend == BackendKind::LlamaCpp {
        // One GGUF header read feeds BOTH gates below (the window ceiling and the memory fit).
        let probed = fetch_dims(&model);

        // Gate 1 — the model's OWN declared context window, which is stricter than the
        // running server's `-c` checked above: a depth the model physically cannot hold
        // is wrong even on a machine with memory to spare.
        if let Some(err) = cliff_window_gate(probed.as_ref().map(|d| d.context_length as u32), &model, needed_ctx, headroom) {
            return Err(err);
        }

        // Gate 2 — this machine's memory at that depth.
        let hw = snapshot();
        if let Some(cap) = device_cap_bytes(hw.gpu.unified, hw.total_memory_bytes, hw.gpu.vram_total_bytes) {
            let installed = installed_models();
            let weights: HashMap<String, u64> = installed.iter().map(|m| (m.name.clone(), m.size_bytes)).collect();
            let w = registry_get(&weights, &model).copied();
            let dims = w.and_then(|_| probed.as_ref()).map(|d| Dims {
                layers: d.layers,
                head_count: d.head_count,
                head_count_kv: d.head_count_kv,
                embedding_length: d.embedding_length,
                context_length: d.context_length as u32,
                kv_estimated: d.kv_estimated,
            });
            if let Some(profile) = try_profile(w, dims, Some(needed_ctx), Some(cap), KvPrecision::F16) {
                if !profile.fits {
                    return Err(AppError::Inference(cliff_vram_msg(&model, needed_ctx, &profile, headroom)));
                }
            }
        }
    }

    // Native tool-calling gate: a model whose template lacks
    // tool support can't run native either — refuse up front (mirrors the batch's
    // `probe_native_tools`) so the user gets a clear "switch to Prompt-based" instead of a
    // ladder of empty tool calls.
    if native && !probe_native_tools(backend, &endpoint_for(backend), &model).await {
        return Err(AppError::Inference(format!(
            "\"{model}\" can't run native tool-calling on this backend — switch the Context Stress Test to Prompt-based, or start llama.cpp with --jinja and a tool-capable model."
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
            // Fallback only — the engine pins the per-rung depth-banded budget on the
            // spec, which wins the merge (see `merge_eval_options`).
            max_tokens: answer_tokens_for(Tier::Easy),
            is_thinking: budget.is_thinking,
        };
        // `needed_ctx` is the window the run asked for and — the gates above having passed —
        // believes it got. A rung whose measured prompt reaches it was truncated, not measured.
        run_cliff_with_factory(&make_native, &model, &tasks, &source, &ladder, &DEFAULT_DEPTHS, needed_ctx, budget, &cancel, &mut on_rung, &mut on_step).await?
    } else {
        let turn = BackendTurn {
            backend,
            endpoint: endpoint_for(backend),
            model: model.clone(),
            cancel: cancel.clone(),
            options: Some(options),
            keep_alive: None,
            // A thinking probe reasons before its call (and gets the depth-banded scratchpad
            // via the engine's per-rung spec budget); non-thinking keeps the answer floor.
            is_thinking: budget.is_thinking,
            max_tokens: answer_tokens_for(Tier::Easy),
            cpu_offloaded: false, // liveness probe, not a scored run — no need to grant extra time
            ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING, // probe: fixed fallback window
            stop_cache: Default::default(),
        };
        run_cliff_with(&turn, &model, &tasks, &source, &ladder, &DEFAULT_DEPTHS, needed_ctx, budget, &cancel, &mut on_rung, &mut on_step).await?
    };

    // Stamp the decoding config the run actually used (greedy 0.0 unless the user's
    // global params set one) — the report must label a sampled depth as sampled.
    let mut report = report;
    report.temperature = effective_temperature;
    let report = report;

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

/// The Agent Report payload: the per-model verdicts plus the right-sizing summary
/// derived from them (the smallest quant of each family still Ready on this
/// hardware, percent-only). `right_sizing_hint` explains an empty summary
/// ("assess ≥2 quants…"). Host-specific — never published.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReadinessAssessment {
    pub verdicts: Vec<ModelVerdict>,
    pub right_sizing: Vec<RightSizingGroup>,
    pub right_sizing_hint: Option<String>,
}

/// Assess the collection's last persisted batch report against a profile. Scoring
/// is `readiness::assess` — the one source of truth shared with the future CLI;
/// this command adds no scoring logic of its own. When `cap_bytes` is set it also
/// measures VRAM fit for each **the server** and **llama.cpp** column (exact weights +
/// real KV cache at the run's `num_ctx` vs the cap; llama.cpp graded at the launch's
/// actual KV precision); the remote server/remote backends and an absent cap leave fit unmeasured
/// (`memory = None`) — never a guessed fit. Empty verdicts means no run has been
/// persisted yet (the page shows an empty state).
#[tauri::command]
pub async fn assess_readiness(
    app: AppHandle,
    collection_id: String,
    profile_id: String,
    cap_bytes: Option<u64>,
) -> Result<ReadinessAssessment, AppError> {
    let profile = profiles::load(&profiles_dir(&app)?, &profile_id)?;
    let report = match reports::load(&reports_dir(&app)?, &collection_id)? {
        Some(r) => r,
        None => return Ok(ReadinessAssessment { verdicts: Vec::new(), right_sizing: Vec::new(), right_sizing_hint: None }),
    };

    // Real model metadata by name, read from the installed GGUF headers: the weight
    // size (for VRAM fit) and the real quantization (for the table — never guessed).
    // Best-effort: with nothing installed the maps are empty and those fields stay
    // N/A rather than failing the assess.
    let installed = installed_models();
    let weights: HashMap<String, u64> = installed.iter().map(|m| (m.name.clone(), m.size_bytes)).collect();
    let quants: HashMap<String, String> =
        installed.iter().filter(|m| !m.quantization.is_empty()).map(|m| (m.name.clone(), m.quantization.clone())).collect();
    // Right-sizing grouping metadata: `(family parameter_size, weights)` per model.
    // Only models with BOTH a family and a size class can be grouped (an empty key
    // would wrongly merge unrelated models) — mirrors the Quant tab's grouping.
    let rs_meta: right_size::ModelMeta = installed
        .iter()
        .filter(|m| !m.family.is_empty() && !m.parameter_size.is_empty())
        .map(|m| (m.name.clone(), (format!("{} {}", m.family, m.parameter_size), m.size_bytes)))
        .collect();

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

    // Real machine total, read ONCE — the llama.cpp gate grades the fit at the KV
    // precision `plan_launch` would ACTUALLY pick on THIS machine (the cap slider
    // simulates a smaller box for the fit itself, but the launch runs here).
    let needs_llama_fit = cap_bytes.is_some() && report.columns.iter().any(|c| c.backend == BackendKind::LlamaCpp);
    let total_memory_bytes =
        needs_llama_fit.then(|| crate::commands::system::hardware::snapshot().total_memory_bytes);

    let mut out = Vec::with_capacity(report.columns.len());
    for col in &report.columns {
        let memory = if cap_bytes.is_some() && col.backend == BackendKind::LlamaCpp {
            let w = registry_get(&weights, &col.model).copied();
            let dims = match w {
                Some(_) => fetch_dims(&col.model).map(|d| Dims {
                    layers: d.layers,
                    head_count: d.head_count,
                    head_count_kv: d.head_count_kv,
                    embedding_length: d.embedding_length,
                    context_length: d.context_length as u32,
                    kv_estimated: d.kv_estimated,
                }),
                None => None,
            };
            // Some servers set cache type via a server-global env var
            // that silently falls back to f16 per-architecture — unverifiable from
            // here, so the gate stays at the f16 default it ships with.
            try_profile(w, dims, report.num_ctx, cap_bytes, KvPrecision::F16)
        } else if col.backend == BackendKind::LlamaCpp {
            total_memory_bytes.and_then(|total| {
                let path = storage_disk::find_installed_gguf(&col.model)?;
                let weights = std::fs::metadata(&path).ok()?.len();
                let meta = crate::inference::gguf::gguf::inspect_gguf(&path).ok()?;
                llama_profile_from_meta(weights, &meta, report.num_ctx, cap_bytes, total)
            })
        } else {
            // servers that expose no KV-quant flags and remote vLLM/SGLang (cache
            // dtype is a server-launch flag we can't verify): no measured fit here.
            None
        };
        let fits_in_vram = memory.as_ref().map(|m| m.fits);
        let vram_pressure = memory.as_ref().map(|m| m.pressure).unwrap_or(false);
        let cliff = registry_get(&cliffs, &col.model).cloned().unwrap_or_default();
        // Real quant: the model registry first, else parsed from the model name (a
        // GGUF/llama.cpp or offline-model) so the row can publish. Model-level —
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
    // Right-sizing summary over the ranked verdicts (dedup keeps the best row per
    // model). Percent-only; host-specific, never published.
    let (right_sizing, right_sizing_hint) = right_size::summarize(&out, &rs_meta);
    Ok(ReadinessAssessment { verdicts: out, right_sizing, right_sizing_hint })
}

#[cfg(test)]
mod cliff_preflight_tests {
    use super::*;

    /// An EMPTY path (the re-probe dropped it) must fail with a DISTINCT "no model path"
    /// error, NOT the misleading "start with a bigger context" WrongModel message — even
    /// though probe_readiness("") returns WrongModel.
    #[test]
    fn empty_path_yields_a_distinct_no_path_error_not_wrong_model() {
        let err = llama_cliff_gate("", LlamaProbeReadiness::WrongModel, "qwen2.5-coder", 6144, CLIFF_CTX_HEADROOM).unwrap();
        let msg = err.to_string();
        assert!(msg.contains("No model path was provided"), "honest distinct error: {msg}");
        assert!(!msg.contains("Context window of at least"), "must NOT be the start-with-model message: {msg}");
    }

    #[test]
    fn llama_gate_maps_readiness_when_path_is_present() {
        // Wrong/absent model → start-with-model message.
        assert!(llama_cliff_gate("/w/m.gguf", LlamaProbeReadiness::WrongModel, "m", 6144, CLIFF_CTX_HEADROOM)
            .unwrap().to_string().contains("Start llama.cpp"));
        assert!(llama_cliff_gate("/w/m.gguf", LlamaProbeReadiness::NotRunning, "m", 6144, CLIFF_CTX_HEADROOM)
            .unwrap().to_string().contains("Start llama.cpp"));
        // Loaded but too small → raise/reduce message.
        assert!(llama_cliff_gate("/w/m.gguf", LlamaProbeReadiness::Ready { ctx: 4096 }, "m", 6144, CLIFF_CTX_HEADROOM)
            .unwrap().to_string().contains("Raise"));
        // Loaded with enough context → no error (the probe proceeds).
        assert!(llama_cliff_gate("/w/m.gguf", LlamaProbeReadiness::Ready { ctx: 8192 }, "m", 6144, CLIFF_CTX_HEADROOM).is_none());
    }

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
        let m = raise_or_reduce_msg(8192, 18_432, CLIFF_CTX_HEADROOM);
        assert!(m.contains("8192"), "names the running window: {m}");
        assert!(m.contains("18432"), "names the needed window: {m}");
        assert!(m.contains("Context window"), "names the raise lever: {m}");
        assert!(m.contains("6144"), "a safe depth of 8192 - 2048 headroom: {m}");
    }

    /// THE REGRESSION. The panel defaulted Max Tokens to the model's FULL context window,
    /// and the probe runs at `max_tokens + CLIFF_CTX_HEADROOM` — so the deepest rung asked
    /// for more context than the model has, for EVERY model. the server does not reject that: it
    /// silently clamps `num_ctx` to the trained window and truncates the prompt, deleting the
    /// injected needle and pinning `prompt_eval_count` at the window. The rung then fails for
    /// a reason the model never caused, and reports a saturated (fabricated) depth as
    /// "verified". Nothing downstream can detect it, so the gate must refuse up front.
    #[test]
    fn cliff_window_gate_refuses_a_depth_the_model_cannot_hold() {
        // Max Tokens = the full 32768 window → needed_ctx = 34816 > 32768. Verified live:
        // the server answers this request with n_ctx = 32768 and a truncated prompt.
        let err = cliff_window_gate(Some(32_768), "qwen2.5:3b", 34_816, CLIFF_CTX_HEADROOM).expect("must refuse");
        let m = err.to_string();
        assert!(m.contains("32768"), "names the model's real window: {m}");
        assert!(m.contains("30720"), "names the deepest Max Tokens that fits (32768 - 2048): {m}");
        assert!(m.contains("qwen2.5:3b"), "names the model: {m}");
    }

    /// The fixed default (window - headroom) must be allowed through — the cap has to
    /// preserve the deepest MEASURABLE depth, not cost the user usable range.
    #[test]
    fn cliff_window_gate_allows_the_deepest_depth_that_fits() {
        // What the capped slider now produces: 30720 + 2048 headroom == the 32768 window.
        assert!(cliff_window_gate(Some(32_768), "m", 30_720 + CLIFF_CTX_HEADROOM, CLIFF_CTX_HEADROOM).is_none());
        assert!(cliff_window_gate(Some(32_768), "m", 8_192, CLIFF_CTX_HEADROOM).is_none());
    }

    /// An unknown window is UNMEASURABLE, never a guessed block — same rule as the VRAM
    /// gate (a missing input must not invent an alarm).
    #[test]
    fn cliff_window_gate_never_blocks_on_an_unknown_window() {
        assert!(cliff_window_gate(None, "m", 999_999, CLIFF_CTX_HEADROOM).is_none());
    }

    fn nineb_meta() -> crate::inference::gguf::gguf::GgufMetadata {
        crate::inference::gguf::gguf::GgufMetadata {
            architecture: "qwen2".into(),
            parameter_count: None,
            context_length: Some(32_768),
            quantization: Some("Q4_K_M".into()),
            family: "qwen".into(),
            block_count: Some(36),
            head_count: Some(40),
            head_count_kv: Some(8),
            embedding_length: Some(5120),
        }
    }

    /// Roomy machine → the launch plan stays f16 and the profile says so.
    #[test]
    fn llama_column_profile_is_f16_on_a_roomy_machine() {
        let p = llama_profile_from_meta(9_000_000_000, &nineb_meta(), Some(8192), Some(64_000_000_000), 128_000_000_000)
            .unwrap();
        assert_eq!(p.kv_precision, crate::inference::vram_math::KvPrecision::F16);
        assert!(!p.estimated, "real head_count_kv → exact");
        assert!(p.fits);
    }

    /// Tight machine (16 GB): plan_launch downgrades to a Q8 cache for the desired
    /// window → the fit is graded at Q8 and the profile carries the precision —
    /// gate-at-actual-KV, self-describing.
    #[test]
    fn llama_column_profile_carries_plan_kv_precision() {
        let p = llama_profile_from_meta(9_000_000_000, &nineb_meta(), Some(16_384), Some(16_000_000_000), 16_000_000_000)
            .unwrap();
        assert_eq!(p.kv_precision, crate::inference::vram_math::KvPrecision::Q8);
        // per-token q8 = 73,728 B → 16,384 ctx = 1,207,959,552 B — half the f16 cache.
        assert_eq!(p.kv_cache_bytes, 1_207_959_552);
    }

    /// Missing dims (hybrid/exotic header) → None: unmeasured, never guessed.
    #[test]
    fn llama_column_profile_is_none_when_dims_are_missing() {
        let mut meta = nineb_meta();
        meta.head_count = None;
        assert!(llama_profile_from_meta(9_000_000_000, &meta, Some(8192), Some(64_000_000_000), 128_000_000_000).is_none());
    }

    /// A missing GGUF on disk resolves to None — the assess branch then leaves the
    /// llama.cpp column's fit unmeasured (a soft Conditional), never a guessed fit.
    #[test]
    fn find_gguf_returns_none_for_a_missing_file() {
        let dir = std::env::temp_dir().join("qm-nonexistent-gguf-dir");
        assert!(storage_disk::find_gguf(&dir, "does-not-exist").is_none());
        // A name that already ends in .gguf but isn't present is also None.
        assert!(storage_disk::find_gguf(&dir, "ghost.gguf").is_none());
    }

    /// A missing head_count_kv falls back to MHA (conservative overestimate) and the
    /// profile is flagged `estimated` so the UI shows "~", never an exact-looking figure.
    #[test]
    fn llama_column_profile_flags_defaulted_kv_heads_as_estimated() {
        let mut meta = nineb_meta();
        meta.head_count_kv = None;
        let p = llama_profile_from_meta(9_000_000_000, &meta, Some(8192), Some(64_000_000_000), 128_000_000_000)
            .unwrap();
        assert!(p.estimated);
    }

    /// LIVE (ignored): the whole llama.cpp fit chain on a REAL GGUF from
    /// `~/.quantamind/gguf` — find_gguf resolves it, the header parses, and the
    /// profile is graded at whatever precision plan_launch picks on THIS machine.
    /// Run: cargo test --lib live_llama_gguf_profile -- --ignored --nocapture
    #[test]
    #[ignore = "reads a real GGUF from ~/.quantamind/gguf"]
    fn live_llama_gguf_profile_grades_at_plan_precision() {
        let dir = storage_disk::gguf_dir();
        let path = storage_disk::find_gguf(&dir, "qwen3.5-9b_q4_k_m").expect("qwen3.5-9b_q4_k_m.gguf installed");
        let weights = std::fs::metadata(&path).unwrap().len();
        let meta = crate::inference::gguf::gguf::inspect_gguf(&path).unwrap();
        let total = crate::commands::system::hardware::snapshot().total_memory_bytes;
        let p = llama_profile_from_meta(weights, &meta, Some(8192), Some(total), total)
            .expect("real GGUF dims must yield a measured profile");
        eprintln!(
            "LIVE gguf profile: arch={} dims=({:?}L/{:?}H/{:?}KV/{:?}E) weights={} kv={} total={} precision={:?} estimated={}",
            meta.architecture, meta.block_count, meta.head_count, meta.head_count_kv, meta.embedding_length,
            p.weights_bytes, p.kv_cache_bytes, p.total_bytes, p.kv_precision, p.estimated
        );
        assert_eq!(p.weights_bytes, weights, "weights are the exact on-disk size");
        assert!(p.kv_cache_bytes > 0);
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
            kv_precision: Default::default(),
        };
        let m = cliff_vram_msg("gemma-3-12b", 16_384, &p, CLIFF_CTX_HEADROOM);
        assert!(m.contains("gemma-3-12b"), "names the model: {m}");
        assert!(m.contains("16384"), "names the needed context: {m}");
        assert!(m.contains("6144"), "an estimated safe depth: {m}");
        assert!(m.to_lowercase().contains("reduce"), "tells the user to reduce Max Tokens: {m}");
    }

    /// Every remaining backend serves the OpenAI tool wire, so the gate admits them all
    /// without a network call — a template lacking a tool grammar is caught later and
    /// labelled honestly rather than pre-judged here.
    #[tokio::test]
    async fn native_tools_gate_admits_every_supported_backend() {
        for b in [BackendKind::LlamaCpp, BackendKind::VLlm, BackendKind::SgLang] {
            assert!(probe_native_tools(b, "", "any-model").await, "{b:?}");
        }
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

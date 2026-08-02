use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::build::sandbox_for;
use crate::inference::eval::agentic::model_turn::ModelTurn;
use crate::inference::eval::agentic::sandbox::DeterministicSandbox;
use crate::inference::eval::agentic::scoring::boundary::BoundaryReport;
use crate::inference::eval::agentic::scoring::report::{AgenticReport, DiagnosticStats, FailureTracker, TopError};
use crate::inference::eval::agentic::runner::{run_agentic_with, AgenticConfig};
use crate::inference::eval::agentic::difficulty::passk::ThinkPreset;
use crate::inference::eval::agentic::spec::{AttackVector, SafetyArm, Tier};
use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::agentic::v2::generator;
use crate::inference::eval::toolcall::eval::{aggregate, trace_one_with, TaskResult, ToolCallReport, TraceResult};
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::params::InferenceParams;
use crate::inference::eval::toolcall::score::verdict_passed;
use crate::inference::eval::toolcall::tasks::{is_agentic, ToolTask};
use crate::inference::eval::run_summary::RunSummary;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

/// The per-task outcome streamed to the UI and cached for the trace debugger.
/// `Deserialize` so the resumable job queue can reload a completed unit's outcome.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskOutcome {
    Single { passed: bool, trace: TraceResult },
    Agentic { report: AgenticReport },
    Error {
        message: String,
        /// The error was a host memory OOM (Metal/CUDA out-of-memory, the server OOM-kill) —
        /// classified HERE, once, so every consumer (UI badge, ceiling suggestion) reads
        /// the same verdict instead of re-matching strings. `#[serde(default)]` so
        /// persisted pre-flag outcomes load as `false`.
        #[serde(default)]
        oom: bool,
    },
}

/// Host memory OOM, by message. Matches the strings the local backends actually emit
/// (llama.cpp "out of memory", macOS Metal `kIOGPUCommandBufferCallbackErrorOutOfMemory`,
/// the "not enough memory" copy `errors.rs` maps) — deliberately NARROW: an ambiguous
/// infra error stays `InfraHost` without the OOM claim.
pub fn is_oom_message(msg: &str) -> bool {
    let low = msg.to_lowercase();
    low.contains("out of memory") || low.contains("outofmemory") || low.contains("not enough memory")
}

/// One finished (model, task) unit — the durable result the resumable queue
/// appends and reloads. `is_native` tags the parallel native-FC pass. Lives in
/// `inference` (not `persistence`) so the run loop can fold it without `inference`
/// importing the persistence queue (the queue imports this, not the reverse).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CompletedUnit {
    pub model: String,
    pub task_id: String,
    pub category: String,
    pub outcome: TaskOutcome,
    pub is_native: bool,
}

/// The VRAM-isolation gate: evict the previous model and assert its VRAM cleared
/// before the next loads. Injected (not a hardcoded call) so the run loop is
/// testable without live HTTP. An `Err` from `unload` is the hard halt.
#[allow(async_fn_in_trait)]
pub trait VramGate {
    async fn unload(&self, model: &str) -> AppResult<()>;
}

/// No isolation (tests / single-model runs).
pub struct NoVramGate;
impl VramGate for NoVramGate {
    async fn unload(&self, _model: &str) -> AppResult<()> {
        Ok(())
    }
}

/// Fold a reloaded completed unit straight into a model's accumulators on resume —
/// no re-run, no `task_done` replay (the Matrix is repainted in bulk upstream).
fn fold_completed(
    unit: &CompletedUnit,
    task: &ToolTask,
    single_tasks: &mut Vec<ToolTask>,
    single_results: &mut Vec<TaskResult>,
    agentic_reports: &mut Vec<AgenticReport>,
    col_error: &mut Option<String>,
) {
    match &unit.outcome {
        TaskOutcome::Agentic { report } => agentic_reports.push(report.clone()),
        TaskOutcome::Single { trace, .. } => {
            single_tasks.push(task.clone());
            single_results.push(TaskResult {
                id: task.id.clone(),
                category: task.category.clone(),
                verdict: trace.verdict.clone(),
                prompt_tokens: trace.prompt_tokens,
            });
        }
        TaskOutcome::Error { message, .. } => *col_error = Some(message.clone()),
    }
}

/// Streaming surface for a batch run. The command layer implements this to
/// `app.emit()` progress; the engine stays Tauri-free. `Send + Sync` so the
/// agentic per-turn pump can forward from a spawned task.
pub trait BatchSink: Send + Sync {
    /// A task is about to run. `is_native` tags the native pass so the UI shows progress the
    /// instant a pass begins (and can reset the per-pass progress bar at the pass boundary).
    fn task_started(&self, model: &str, task_id: &str, index: usize, total: usize, category: &str, is_native: bool);
    /// A live agentic turn. `is_native` distinguishes the NATIVE function-calling pass from
    /// the prompt pass so the UI can render the two trajectories separately (both stream to
    /// the same (model, task) cell).
    fn agentic_turn(&self, model: &str, task_id: &str, step: &TrajectoryStep, is_native: bool);
    /// A task's terminal outcome. `is_native` tags the NATIVE pass's per-task result so the UI
    /// can show native pass/fail in its own column, streamed as each native task finishes.
    fn task_done(&self, model: &str, task_id: &str, outcome: &TaskOutcome, is_native: bool);
}

/// Phase 9: a model's strict Pass^k within ONE difficulty tier. `by_tier` carries
/// these so the readiness gate can derive the highest tier the model actually
/// cleared (`pass_k() >= profile.min_pass_k`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TierStat {
    pub tier: Tier,
    pub tasks_passed: u32,
    pub tasks_total: u32,
    /// Phase 9B: mean steps across this tier's runs — the Agent Report's Tier Progression
    /// Matrix reads it. `None` when no run produced steps. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub avg_steps: Option<f64>,
    /// Phase 9B: failure breakdown summed within this tier — the Failure Taxonomy reads it
    /// per tier. `#[serde(default)]` so pre-9B reports (no per-tier failures) still load.
    #[serde(default)]
    pub failures: FailureTracker,
}

impl TierStat {
    /// Strict Pass^k within this tier, or `None` when the tier had no task.
    pub fn pass_k(&self) -> Option<f64> {
        (self.tasks_total > 0).then(|| self.tasks_passed as f64 / self.tasks_total as f64)
    }
}

/// Per-model aggregate of the collection's agentic tasks: Pass^k, mean
/// steps/effort, dominant failure. Null metrics render "N/A", never fabricated.
///
/// Pass^k semantics (spec §3.3): a task is credited only when **all k of its runs
/// succeed** — reliability compounds, so a model that passes 3/5 on a task is not
/// "60% reliable", it is unreliable. `tasks_passed`/`tasks_total` carry that strict
/// metric; `passes`/`total_runs` keep the run-level sums for the secondary per-run
/// rate (pass@k) shown alongside the Partial badge.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AggAgentic {
    /// Tasks where every one of the k runs reached the end state (strict Pass^k numerator).
    #[serde(default)]
    pub tasks_passed: u32,
    /// Agentic tasks aggregated into this column (strict Pass^k denominator).
    #[serde(default)]
    pub tasks_total: u32,
    /// Run-level sums across all tasks — the secondary per-run rate (pass@k), NOT the headline.
    pub passes: u32,
    pub total_runs: u32,
    pub avg_steps: Option<f64>,
    pub avg_output_tokens_success: Option<f64>,
    /// Driver D: mean per-task schema resilience over this model's tasks that hit a
    /// schema error. `None` when none did → the Matrix renders "—", never a 0.
    pub schema_resilience: Option<f64>,
    pub top_error: TopError,
    /// Summed failure breakdown across this model's agentic tasks. The readiness
    /// verdict gates on the exact loop/hallucination counts — `top_error` alone
    /// would hide a 1-loop/9-hallucination model from a `forbid_infinite_loop` profile.
    #[serde(default)]
    pub failures: FailureTracker,
    /// Phase 9: per-tier strict Pass^k breakdown (sorted ascending by tier). Empty
    /// for pre-Phase-9 reports. The readiness gate reads this to compute the highest
    /// difficulty tier the model cleared. `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub by_tier: Vec<TierStat>,
    /// Native-FC only: tasks whose every run ERRORED (a backend `Err`, not a scored
    /// failure) and so produced no report. Carried SEPARATELY from `tasks_total` (the
    /// scored denominator) so `pass_k` is never diluted by infra — the shrink is visible,
    /// not silent: a column reads "0/3 scored, 2 errored"; attempted = total + errored.
    #[serde(default)]
    pub tasks_errored: u32,
    /// What KIND of error the errored tasks hit. An infra/host crash (`InfraHost`) must
    /// never read as model incapability — only a native-path schema rejection does.
    #[serde(default)]
    pub native_error_class: NativeErrorClass,
    /// Category K: the safety/boundary aggregate over this path's Category-K tasks, when the
    /// collection carries any. `None` for a capability-only collection — never a fabricated 0.
    /// Kept OUT of `pass_k`/composite so the two metrics are never blended; sitting on the
    /// per-path aggregate keeps prompt-vs-native structurally separate.
    #[serde(default)]
    pub boundary: Option<BoundaryReport>,
    /// Summed over this path's tasks: turns whose native tool API returned STRUCTURED
    /// `tool_calls` vs turns where it returned none and the calls were salvaged out of the
    /// `content` text. `None` = the prompt path, or an aggregate written before this was
    /// measured; `Some(0)` = we asked the native API and it never once returned a structured
    /// call, which means this path's score was produced entirely by the text salvager and is
    /// NOT native function-calling (see `inputs::measured_native`).
    #[serde(default)]
    pub native_structured_calls: Option<u32>,
    #[serde(default)]
    pub native_salvaged_calls: Option<u32>,
    /// T*: tokens-per-completed-task — total generated tokens over every run ÷ completions
    /// (run-weighted). The amortized cost including tokens wasted on failed runs, so
    /// `>= avg_output_tokens_success` (Effort). `None` when nothing completed. Output tokens
    /// only; per-path (never blended across native/prompt). `#[serde(default)]` so older
    /// reports load.
    #[serde(default)]
    pub tokens_per_completed: Option<f64>,
    /// Bring-Your-Own: the per-model diagnostic summary (schema-valid rate + fault
    /// attribution) for a no-answer-key run against the user's own server. `Some` ONLY
    /// on a BYO column; the Model-Results row then shows "schema-valid X/Y", never a
    /// pass^k. Kept OUT of `pass_k` so the two are never blended. `#[serde(default)]`.
    #[serde(default)]
    pub diagnostic: Option<DiagnosticStats>,
}

/// Why a native-FC task produced no scored result (every run errored). Kept distinct from
/// model FAILURES (which are scored) so an infra/host error is never read as incapability —
/// the misattribution the timeout finding warned about, one layer down.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NativeErrorClass {
    /// No task errored.
    #[default]
    None,
    /// Backend/transport error (the server OOM-killed or crashed under host pressure, a 5xx, a
    /// dropped connection, a transport timeout). NOT a capability signal — the machine.
    InfraHost,
    /// The native tool API rejected the request (a 4xx): this model can't express the
    /// task's tool schema natively. A legitimate "can't run native", not host pressure.
    SchemaRejected,
    /// Both kinds occurred across the errored tasks.
    Mixed,
}

/// Classify a native-pass task error. A 4xx from `/api/chat` means the request/tool-schema
/// was rejected (`SchemaRejected`); everything else — 5xx, connection lost, transport
/// timeout — is `InfraHost` and must never imply model incapability. Defaults to
/// `InfraHost` on anything ambiguous (the safe direction: blame the machine, not the model).
fn classify_native_error(msg: &str) -> NativeErrorClass {
    if msg.contains("HTTP 4") {
        NativeErrorClass::SchemaRejected
    } else {
        NativeErrorClass::InfraHost
    }
}

/// Fold a task's error class into the column's running class (`None` is identity; two
/// different non-`None` classes become `Mixed`).
fn merge_error_class(acc: NativeErrorClass, next: NativeErrorClass) -> NativeErrorClass {
    match (acc, next) {
        (NativeErrorClass::None, x) | (x, NativeErrorClass::None) => x,
        (a, b) if a == b => a,
        _ => NativeErrorClass::Mixed,
    }
}

impl AggAgentic {
    /// Strict Pass^k: fraction of tasks whose every run succeeded. `None` when no
    /// task was aggregated (the row then renders "N/A", never a fabricated 0).
    pub fn pass_k(&self) -> Option<f64> {
        (self.tasks_total > 0).then(|| self.tasks_passed as f64 / self.tasks_total as f64)
    }
}

/// One model's row in the Matrix Scoreboard: single-turn report and/or agentic
/// aggregate (whichever the collection contained), or the error it hit.
/// `Default` exists so constructors can close with `..Default::default()` — a new
/// stamped-fact field then lands everywhere without touching every literal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct BatchColumn {
    pub model: String,
    pub backend: BackendKind,
    pub toolcall: Option<ToolCallReport>,
    pub agentic: Option<AggAgentic>,
    /// Phase 7.2: the parallel NATIVE function-calling aggregate (the OpenAI tool wire
    /// `tool_calls`), when measured. `None` = not run / unsupported backend → N/A.
    /// `#[serde(default)]` so pre-7.2 reports still load.
    #[serde(default)]
    pub agentic_native_fc: Option<AggAgentic>,
    pub error: Option<String>,
    /// This model ran as a reasoning model (sidebar "thinking" toggle). A thinking model
    /// legitimately spends far more output tokens (the `<think>` scratchpad), so its
    /// `effort`/`avg_output_tokens` is NOT comparable to a terse model's — the UI must show
    /// this flag rather than rank the two on tokens. `#[serde(default)]` so older reports
    /// (and non-thinking columns) load as `false`.
    #[serde(default)]
    pub is_thinking: bool,
    /// This model's turns were CPU-offloaded (the server spilled it past VRAM) — a slow-inference
    /// signal the runner uses to grant a larger per-step budget. Surfaced so a slow verdict reads
    /// as "offloaded", not "incapable". `#[serde(default)]` so older reports load as `false`.
    #[serde(default)]
    pub cpu_offloaded: bool,
    /// The hardware-adaptive `num_ctx` ceiling this model actually ran under (bigger box → bigger
    /// window; llama.cpp clamps to its launched `-c`). The one knob hardware moves. `None` when not
    /// recorded / older report. `#[serde(default)]`.
    #[serde(default)]
    pub ctx_ceiling: Option<u32>,
    /// Weight placement measured from `/api/ps` at run start (the server only; `None` elsewhere /
    /// older reports). `offload_bytes` = size − size_vram — the measured CPU spill QUANTITY
    /// behind `cpu_offloaded` (the "why 3 tok/s" answer). `weights_vram_bytes` is the constant
    /// weights baseline the Inspector's memory breakdown stacks under the per-task KV cost.
    #[serde(default)]
    pub weights_total_bytes: Option<u64>,
    #[serde(default)]
    pub weights_vram_bytes: Option<u64>,
    #[serde(default)]
    pub offload_bytes: Option<u64>,
    /// The quantization `/api/ps` CLAIMS for the loaded model (e.g. "Q4_K_M") — the tag's
    /// assertion, never verified truth. Part of the run-config stamp so a later run-comparison
    /// view is a view, not a migration. `None` when unreported (llama.cpp, older reports).
    #[serde(default)]
    pub quantization_claimed: Option<String>,
    /// KV-cache precision the LOCAL llama-server was launched with ("f16" | "q8_0"), from the
    /// stored `LaunchPlan`. `None` for other backends, an externally-started server (we can't
    /// know its flags — never guess), or older reports.
    #[serde(default)]
    pub kv_cache_type: Option<String>,
}
fn is_zero(n: &usize) -> bool {
    *n == 0
}


/// The full batch result: one column per target model.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BatchReport {
    pub collection_id: String,
    pub columns: Vec<BatchColumn>,
    /// Stored columns the loader couldn't interpret (e.g. one recorded against a
    /// backend this build no longer supports). Skipped rather than failing the
    /// whole report, and counted so the UI can say what it isn't showing — a
    /// short verdict table must never read as the complete run. Always 0 for a
    /// freshly-produced report; `#[serde(default)]` + skip so it never bloats
    /// what we persist or publish.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreadable_columns: usize,
    /// Wall-clock→dollars for this run, when a price basis was declared in Settings.
    /// Computed by the ONE shared implementation (`eval::costs`) that `qm --costs`
    /// uses, so the app's Test-run view and the CLI can never drift. `None` when no
    /// price is set — the UI then reads "n/a (no price basis)", never $0.00.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costs: Option<crate::inference::eval::costs::RunCostSummary>,
    /// The context length (`num_ctx`) the run used, when set — the basis for the
    /// readiness VRAM-fit KV-cache estimate. `#[serde(default)]` so reports saved
    /// before Phase 7.4 (and the engine, which doesn't know the param) still load.
    #[serde(default)]
    pub num_ctx: Option<u32>,
    /// The leaderboard identity hash for THIS run — content-verified at run time: `Some(hash)`
    /// ONLY when the run's tasks are byte-for-byte the pristine bundled collection; `None` for a
    /// custom/imported collection OR any edit (the fork-on-edit guard). Publish reads THIS (never
    /// re-derives from `collection_id`), so an edited/doctored collection can't publish under a
    /// real bundled identity. `#[serde(default)]` so older reports load (as `None` = unpublishable).
    #[serde(default)]
    pub collection_hash: Option<String>,
    /// The Thinking-Budget preset this batch ran under (reasoning models' scratchpad allowance —
    /// Lean/Standard/Deep). Batch-wide, like `num_ctx`. `None` for a report saved before this field
    /// existed. `#[serde(default)]`.
    #[serde(default)]
    pub think_preset: Option<ThinkPreset>,
    /// The full inference params THIS batch ran with, stamped at run time like `num_ctx` —
    /// publish reads THESE (never the live global header, which the user may have edited since
    /// the run). `None` = the run sent no params (backend defaults) or a pre-this-field report.
    #[serde(default)]
    pub params: Option<InferenceParams>,
}

fn mean_f64(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

/// Per-model history rows for the Audit timeline: the single-turn composite plus
/// the agentic Pass^k / steps / effort, for every model whose column didn't error.
pub fn batch_summaries(report: &BatchReport, ts: &str) -> Vec<RunSummary> {
    report
        .columns
        .iter()
        .filter(|c| c.error.is_none())
        .map(|c| {
            let tc = c.toolcall.as_ref();
            let ag = c.agentic.as_ref();
            RunSummary {
                ts: ts.to_string(),
                model: c.model.clone(),
                backend: c.backend,
                parse_rate: tc.and_then(|r| r.parse_rate),
                tool_selection_acc: tc.and_then(|r| r.tool_selection_acc),
                arg_acc: tc.and_then(|r| r.arg_acc),
                abstain_acc: tc.and_then(|r| r.abstain_acc),
                composite: tc.and_then(|r| r.composite),
                n: tc.map(|r| r.n).unwrap_or(0),
                pass_k: ag.and_then(|a| a.pass_k()),
                agentic_avg_steps: ag.and_then(|a| a.avg_steps),
                effort: ag.and_then(|a| a.avg_output_tokens_success),
                is_thinking: c.is_thinking,
            }
        })
        .collect()
}

/// Sum an optional counter across reports, preserving "never measured". All-`None` stays
/// `None`; any `Some` makes the result `Some` — so a zero in the output can only ever have
/// come from a real measurement, never from absence.
fn sum_opt(it: impl Iterator<Item = Option<u32>>) -> Option<u32> {
    it.fold(None, |acc, v| match (acc, v) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    })
}

fn agg_agentic(reports: &[AgenticReport], native_fc: bool) -> AggAgentic {
    let mut failures = FailureTracker::default();
    for r in reports {
        failures.merge(&r.failures); // centralized — never drops a field (e.g. unknown/forbidden)
    }
    let top_error = failures.top();
    let steps: Vec<f64> = reports.iter().filter_map(|r| r.avg_steps).collect();
    let eff: Vec<f64> = reports.iter().filter_map(|r| r.avg_output_tokens_success).collect();
    let resil: Vec<f64> = reports.iter().filter_map(|r| r.schema_resilience).collect();
    // Phase 9 (Gap 2): bucket strict Pass^k by tier. A HashMap keeps this generic
    // over whatever tiers exist (no hardcoded per-tier arms); the output is sorted
    // by tier so the readiness gate can walk it highest-first.
    let mut buckets: HashMap<Tier, Vec<&AgenticReport>> = HashMap::new();
    for r in reports {
        buckets.entry(r.tier).or_default().push(r);
    }
    let mut by_tier: Vec<TierStat> = buckets
        .into_iter()
        .map(|(tier, rs)| {
            // Phase 9B: per-tier avg steps + failures, computed exactly like the overall
            // fields but scoped to this tier's bucket (the Agent Report renders both).
            let tier_steps: Vec<f64> = rs.iter().filter_map(|r| r.avg_steps).collect();
            let mut tier_failures = FailureTracker::default();
            for r in &rs {
                tier_failures.merge(&r.failures);
            }
            TierStat {
                tier,
                tasks_passed: rs.iter().filter(|r| r.is_strict_pass()).count() as u32,
                tasks_total: rs.len() as u32,
                avg_steps: mean_f64(&tier_steps),
                failures: tier_failures,
            }
        })
        .collect();
    by_tier.sort_by_key(|s| s.tier);
    // T* (tokens per completed task), run-weighted so it's the honest amortized cost:
    // total generated tokens across every run ÷ total completed runs. `None` when nothing
    // completed (never a fabricated 0). Unlike Effort (mean-of-per-task-success-means), this
    // charges the tokens wasted on failed runs to the completions that landed.
    let total_output_tokens: u64 = reports.iter().map(|r| r.output_tokens_total as u64).sum();
    let total_passes: u64 = reports.iter().map(|r| r.passes as u64).sum();
    let tokens_per_completed = (total_passes > 0).then(|| total_output_tokens as f64 / total_passes as f64);
    AggAgentic {
        tasks_passed: reports.iter().filter(|r| r.is_strict_pass()).count() as u32,
        tasks_total: reports.len() as u32,
        passes: reports.iter().map(|r| r.passes).sum(),
        total_runs: reports.iter().map(|r| r.total_runs).sum(),
        avg_steps: mean_f64(&steps),
        avg_output_tokens_success: mean_f64(&eff),
        schema_resilience: mean_f64(&resil),
        top_error,
        failures,
        by_tier,
        tasks_errored: 0,
        native_error_class: NativeErrorClass::None,
        // Category K: fold the safety-probe subset into the per-path boundary aggregate
        // (None when the collection carries no Category-K tasks). `native_fc` keeps the
        // prompt and native aggregates un-blendable downstream.
        boundary: BoundaryReport::from_reports(reports, native_fc),
        tokens_per_completed,
        // Fold the channel tally. `None` unless a report actually measured it, so an
        // aggregate over pre-existing reports stays "not recorded" rather than a fake zero.
        native_structured_calls: sum_opt(reports.iter().map(|r| r.native_structured_calls)),
        native_salvaged_calls: sum_opt(reports.iter().map(|r| r.native_salvaged_calls)),
        diagnostic: None, // world/built-in aggregate; the BYO adapter builds its own column
    }
}

/// The difficulty tier a task declares (Easy for a single-turn or pre-Phase-9 task).
fn task_tier(task: &ToolTask) -> Tier {
    task.agentic.as_ref().map(|a| a.tier).unwrap_or_default()
}

/// The Category-K safety classification a task declares (arm + vector), or `None` for a
/// capability task.
fn task_safety(task: &ToolTask) -> Option<(SafetyArm, AttackVector)> {
    task.agentic.as_ref().and_then(|a| a.safety.as_ref()).map(|s| (s.arm, s.attack))
}

/// Stamp the per-task metadata `agg_agentic` reads off each report: the difficulty tier
/// (always) and the safety classification (Category-K tasks only). Centralized so both
/// the prompt and native-FC paths stamp identically.
fn stamp_task_meta(report: AgenticReport, task: &ToolTask) -> AgenticReport {
    let report = report.with_tier(task_tier(task));
    match task_safety(task) {
        Some((arm, attack)) => report.with_safety(arm, attack),
        None => report,
    }
}

/// Run one agentic task, forwarding its live `TrajectoryStep`s to the sink as
/// they arrive (a spawned pump drains the channel concurrently with the run).
async fn run_one_agentic<M: ModelTurn + Send + Sync>(
    turn: &M,
    task: &ToolTask,
    model: &str,
    cancel: &CancellationToken,
    sink: Arc<dyn BatchSink>,
    budget_scale: usize,
) -> AppResult<AgenticReport> {
    let (sandbox, cfg) = sandbox_for(task)?;
    let (tx, mut rx) = unbounded_channel::<TrajectoryStep>();
    let (s2, model2, task2) = (sink.clone(), model.to_string(), task.id.clone());
    let pump = tokio::spawn(async move {
        while let Some(step) = rx.recv().await {
            s2.agentic_turn(&model2, &task2, &step, false); // prompt pass
        }
    });
    let started = std::time::Instant::now();
    let result = run_agentic_for(turn, task, model, &sandbox, cfg, cancel, budget_scale, &tx).await;
    drop(tx);
    let _ = pump.await;
    // Whole-batch wall-clock (all k runs, model + sandbox/world time) — the per-turn server
    // timings can't provide this, they exclude everything between turns.
    result.map(|r| stamp_task_meta(r, task).with_wall_ms(started.elapsed().as_millis() as u64))
}

/// Drive Pass^k for a task: a `generated` task builds a FRESH procedural instance
/// per run (seeded by model + run_index → contamination resistance); a static task
/// reuses the one `sandbox`. The shared seam both run paths (streaming + native FC)
/// call so generation behaves identically in each.
#[allow(clippy::too_many_arguments)]
async fn run_agentic_for<M: ModelTurn>(
    turn: &M,
    task: &ToolTask,
    model: &str,
    sandbox: &DeterministicSandbox,
    cfg: AgenticConfig,
    cancel: &CancellationToken,
    budget_scale: usize,
    tx: &tokio::sync::mpsc::UnboundedSender<TrajectoryStep>,
) -> AppResult<AgenticReport> {
    let generated = task.agentic.as_ref().map(|s| s.generated).unwrap_or(false);
    run_agentic_with(
        turn,
        cfg.k,
        |run_index| {
            if generated {
                let inst = generator::instantiate(task, generator::seed_for(model, run_index));
                let (sb, c) = sandbox_for(&inst)?;
                Ok((sb, c.max_steps, c.max_recovery))
            } else {
                Ok((sandbox.clone(), cfg.max_steps, cfg.max_recovery))
            }
        },
        cancel,
        budget_scale,
        tx,
    )
    .await
}

/// The non-resumable dispatcher (tests, no hardware gate): a thin wrapper over
/// `run_batch_resumable` with no prior units, a no-op recorder, and VRAM isolation
/// off — byte-identical to the pre-7.5 behaviour.
pub async fn run_batch<M, F>(
    collection_id: &str,
    targets: &[ModelTarget],
    tasks: &[ToolTask],
    cancel: CancellationToken,
    sink: Arc<dyn BatchSink>,
    make_turn: F,
) -> AppResult<BatchReport>
where
    M: ModelTurn + Send + Sync,
    F: Fn(&ModelTarget) -> M,
{
    run_batch_resumable(collection_id, targets, tasks, cancel, sink, make_turn, &[], &|_| {}, &NoVramGate, 1).await
}

/// The VRAM-safe, **resumable** sequential dispatcher. For each target model:
/// (1) the **VRAM-isolation gate** unloads the previous model and asserts
/// its VRAM cleared before this one loads — an `Err` here propagates and **halts**
/// the run with the job log intact (never loads onto dirty VRAM); (2) every task
/// runs in order — a unit already in `prior` is **folded silently** (no re-run, no
/// `task_done` replay), others run, stream through `sink`, and are handed to
/// `record` (the durable append). ONE model runs ONE task at a time.
#[allow(clippy::too_many_arguments)]
pub async fn run_batch_resumable<M, F, G>(
    collection_id: &str,
    targets: &[ModelTarget],
    tasks: &[ToolTask],
    cancel: CancellationToken,
    sink: Arc<dyn BatchSink>,
    make_turn: F,
    prior: &[CompletedUnit],
    record: &(dyn Fn(&CompletedUnit) + Sync),
    gate: &G,
    concurrency: usize,
) -> AppResult<BatchReport>
where
    M: ModelTurn + Send + Sync,
    F: Fn(&ModelTarget) -> M,
    G: VramGate,
{
    // Completed prompt-pass units, keyed by (model, task) — skipped on resume.
    let done: HashMap<(&str, &str), &CompletedUnit> = prior
        .iter()
        .filter(|u| !u.is_native)
        .map(|u| ((u.model.as_str(), u.task_id.as_str()), u))
        .collect();
    let mut columns = Vec::with_capacity(targets.len());
    let mut prev: Option<(String, BackendKind)> = None;
    for target in targets {
        // VRAM-isolation gate: evict the previous model and confirm VRAM freed
        // before this one loads. Assert-and-fail — Err halts (log intact).
        if let Some((pm, _)) = &prev {
            if pm != &target.model {
                gate.unload(pm).await?;
            }
        }
        let turn = make_turn(target);
        // Warm the model resident before its first SCORED task, so cold-load latency
        // (weights into VRAM) isn't charged to that task as a TurnTimeout. Best-effort:
        // a warm-up error isn't fatal — the first real task will surface a genuine fault.
        let _ = turn.warm_up().await;
        let mut single_tasks: Vec<ToolTask> = Vec::new();
        let mut single_results: Vec<TaskResult> = Vec::new();
        let mut agentic_reports: Vec<AgenticReport> = Vec::new();
        let mut col_error: Option<String> = None;

        // Per-task COMPUTE outcome (the model call), produced under bounded concurrency
        // (`concurrency`) but COMMITTED below in TASK-INDEX order: `.buffered(n)` yields items
        // in input order regardless of completion order, so N=1 is byte-identical to the prior
        // serial loop and N>1 still commits deterministically. The resume-skip (`done` fold)
        // and the cancellation check run INSIDE each task's future, at ITS start — so a
        // cancelled/folded task never makes a model call, matching the old driving-loop checks.
        enum StepOutcome {
            Cancelled,
            Folded,
            Agentic(AppResult<AgenticReport>),
            Single(AppResult<TraceResult>),
        }
        let n_tasks = tasks.len();
        let mut compute = stream::iter(tasks.iter().cloned().enumerate())
            .map(|(i, task)| {
                let turn = &turn;
                let cancel = &cancel;
                let sink = sink.clone();
                let done = &done;
                async move {
                    if cancel.is_cancelled() {
                        return (i, task, StepOutcome::Cancelled);
                    }
                    if done.contains_key(&(target.model.as_str(), task.id.as_str())) {
                        return (i, task, StepOutcome::Folded);
                    }
                    sink.task_started(&target.model, &task.id, i, n_tasks, &task.category, false);
                    if is_agentic(&task.category) {
                        let r = run_one_agentic(turn, &task, &target.model, cancel, sink.clone(), concurrency).await;
                        (i, task, StepOutcome::Agentic(r))
                    } else {
                        let r = trace_one_with(turn, &target.model, &task).await;
                        (i, task, StepOutcome::Single(r))
                    }
                }
            })
            .buffered(concurrency.max(1));

        while let Some((i, task, outcome)) = compute.next().await {
            let _ = i;
            match outcome {
                // Mirrors the old driving loop's `break` on cancel: stop committing further
                // tasks (already in-flight compute for later indices is dropped, not committed).
                StepOutcome::Cancelled => break,
                StepOutcome::Folded => {
                    let unit = done.get(&(target.model.as_str(), task.id.as_str())).expect("checked in compute");
                    fold_completed(unit, &task, &mut single_tasks, &mut single_results, &mut agentic_reports, &mut col_error);
                }
                StepOutcome::Agentic(Ok(report)) => {
                    let outcome = TaskOutcome::Agentic { report: report.clone() };
                    record(&unit_of(target, &task, outcome.clone(), false));
                    sink.task_done(&target.model, &task.id, &outcome, false);
                    agentic_reports.push(report);
                }
                StepOutcome::Agentic(Err(e)) => {
                    // Errors are NOT recorded → they re-run on resume (the backend may be back).
                    let msg = e.to_string();
                    sink.task_done(&target.model, &task.id, &TaskOutcome::Error { message: msg.clone(), oom: is_oom_message(&msg) }, false);
                    col_error = Some(msg);
                }
                StepOutcome::Single(Ok(trace)) => {
                    let passed = verdict_passed(&trace.verdict);
                    single_results.push(TaskResult {
                        id: task.id.clone(),
                        category: task.category.clone(),
                        verdict: trace.verdict.clone(),
                        prompt_tokens: trace.prompt_tokens,
                    });
                    let outcome = TaskOutcome::Single { passed, trace };
                    record(&unit_of(target, &task, outcome.clone(), false));
                    sink.task_done(&target.model, &task.id, &outcome, false);
                    single_tasks.push(task);
                }
                StepOutcome::Single(Err(e)) => {
                    let msg = e.to_string();
                    sink.task_done(&target.model, &task.id, &TaskOutcome::Error { message: msg.clone(), oom: is_oom_message(&msg) }, false);
                    col_error = Some(msg);
                }
            }
        }
        drop(compute);

        let toolcall = (!single_results.is_empty()).then(|| aggregate(&single_tasks, single_results));
        let agentic = (!agentic_reports.is_empty()).then(|| agg_agentic(&agentic_reports, false));
        columns.push(BatchColumn {
            model: target.model.clone(),
            backend: target.backend,
            toolcall,
            agentic,
            agentic_native_fc: None, // filled by run_native_fc_pass when enabled
            error: col_error,
            is_thinking: target.is_thinking,
            // Placement/config facts are stamped by the command layer (they need the probes).
            ..Default::default()
        });
        prev = Some((target.model.clone(), target.backend));
    }
    // The engine is param-agnostic; the command layer stamps `num_ctx`/reasoning
    // budget after.
    Ok(BatchReport { collection_id: collection_id.to_string(), columns, unreadable_columns: 0, costs: None, num_ctx: None, collection_hash: None, think_preset: None, params: None })
}

/// Build a partial `BatchReport` from already-completed units ONLY — no execution.
/// Used on resume to repaint the Matrix in one payload (bulk rehydration) before
/// the live run continues. Folds both prompt units (agentic/single) and native
/// units (`agentic_native_fc`).
pub fn fold_report(
    collection_id: &str,
    targets: &[ModelTarget],
    tasks: &[ToolTask],
    prior: &[CompletedUnit],
) -> BatchReport {
    let prompt: HashMap<(&str, &str), &CompletedUnit> =
        prior.iter().filter(|u| !u.is_native).map(|u| ((u.model.as_str(), u.task_id.as_str()), u)).collect();
    let native: HashMap<(&str, &str), &CompletedUnit> =
        prior.iter().filter(|u| u.is_native).map(|u| ((u.model.as_str(), u.task_id.as_str()), u)).collect();
    let columns = targets
        .iter()
        .map(|target| {
            let mut single_tasks = Vec::new();
            let mut single_results = Vec::new();
            let mut agentic_reports = Vec::new();
            let mut native_reports = Vec::new();
            let mut col_error = None;
            for task in tasks {
                if let Some(u) = prompt.get(&(target.model.as_str(), task.id.as_str())) {
                    fold_completed(u, task, &mut single_tasks, &mut single_results, &mut agentic_reports, &mut col_error);
                }
                if let Some(u) = native.get(&(target.model.as_str(), task.id.as_str())) {
                    if let TaskOutcome::Agentic { report } = &u.outcome {
                        native_reports.push(report.clone());
                    }
                }
            }
            BatchColumn {
                model: target.model.clone(),
                backend: target.backend,
                toolcall: (!single_results.is_empty()).then(|| aggregate(&single_tasks, single_results)),
                agentic: (!agentic_reports.is_empty()).then(|| agg_agentic(&agentic_reports, false)),
                agentic_native_fc: (!native_reports.is_empty()).then(|| agg_agentic(&native_reports, true)),
                error: col_error,
                is_thinking: target.is_thinking,
                ..Default::default() // placement/config facts stamped by the command layer
            }
        })
        .collect();
    BatchReport { collection_id: collection_id.to_string(), columns, unreadable_columns: 0, costs: None, num_ctx: None, collection_hash: None, think_preset: None, params: None }
}

fn unit_of(target: &ModelTarget, task: &ToolTask, outcome: TaskOutcome, is_native: bool) -> CompletedUnit {
    CompletedUnit {
        model: target.model.clone(),
        task_id: task.id.clone(),
        category: task.category.clone(),
        outcome,
        is_native,
    }
}

/// Phase 7.2: measure NATIVE function-calling per model and fold a parallel
/// `agentic_native_fc` aggregate onto each column — the same agentic tasks, the
/// same sandbox/scoring, but driven by `make_native` (the OpenAI tool wire tools in
/// production, a scripted turn in tests). Only the server columns whose model is in
/// `supported` (the capability probe ran upstream) get a native run; others stay
/// `None` (N/A). Native steps ARE streamed to the UI sink (tagged `is_native`) so the user can
/// watch the native trajectory in the Evaluator. Best-effort: a native run that errors leaves
/// the column `None` rather than failing the report.
#[allow(clippy::too_many_arguments)]
pub async fn run_native_fc_pass<M, F, G>(
    report: &mut BatchReport,
    tasks: &[ToolTask],
    supported: &std::collections::HashSet<String>,
    cancel: CancellationToken,
    make_native: F,
    prior: &[CompletedUnit],
    record: &(dyn Fn(&CompletedUnit) + Sync),
    gate: &G,
    sink: Arc<dyn BatchSink>,
    concurrency: usize,
) -> AppResult<()>
where
    M: ModelTurn + Send + Sync,
    F: Fn(&str, &ToolTask) -> M,
    G: VramGate,
{
    let agentic_tasks: Vec<&ToolTask> = tasks.iter().filter(|t| is_agentic(&t.category)).collect();
    if agentic_tasks.is_empty() {
        return Ok(());
    }
    // Completed NATIVE units, keyed by (model, task) — skipped on resume so an
    // overnight native pass resumes where it left off, not from scratch.
    let done: HashMap<(&str, &str), &CompletedUnit> = prior
        .iter()
        .filter(|u| u.is_native)
        .map(|u| ((u.model.as_str(), u.task_id.as_str()), u))
        .collect();
    let mut prev: Option<String> = None;
    for col in report.columns.iter_mut() {
        if cancel.is_cancelled() {
            break;
        }
        // `supported` already holds only native-capable models for THIS run's backend
        // (resolved by `probe_native_tools`), so membership is the whole gate — native
        // FC follows the running server, not a hardcoded the server check.
        if !supported.contains(&col.model) {
            continue;
        }
        // Same VRAM-isolation gate between native model runs (assert-and-fail).
        if let Some(pm) = &prev {
            if pm != &col.model {
                gate.unload(pm).await?;
            }
        }
        let mut reports: Vec<AgenticReport> = Vec::new();
        let mut errored: u32 = 0; // tasks whose every run errored (a backend Err)
        let mut error_class = NativeErrorClass::None;

        // Same bounded-concurrency-compute / index-ordered-commit split as the prompt path
        // (`run_batch_resumable`) — see its comment. N=1 is byte-identical to the prior serial
        // native loop; N>1 still commits in task-index order.
        enum StepOutcome {
            Cancelled,
            // A folded prior unit — `Some(report)` when the prior outcome was Agentic (the only
            // kind ever recorded for a native pass), `None` for any other prior outcome kind
            // (skip the commit entirely, mirroring the old loop's unconditional `continue`).
            Folded(Option<AgenticReport>),
            Ran { result: AppResult<AgenticReport>, wall_ms: u64 },
        }
        let model = col.model.clone();
        let n_agentic_tasks = agentic_tasks.len();
        let owned_agentic_tasks: Vec<ToolTask> = agentic_tasks.iter().map(|t| (*t).clone()).collect();
        let mut compute = stream::iter(owned_agentic_tasks.into_iter().enumerate())
            .map(|(i, task)| {
                let cancel = &cancel;
                let sink = sink.clone();
                let done = &done;
                let model = &model;
                let make_native = &make_native;
                async move {
                    if cancel.is_cancelled() {
                        return (i, task, StepOutcome::Cancelled);
                    }
                    if let Some(unit) = done.get(&(model.as_str(), task.id.as_str())) {
                        let report = match &unit.outcome {
                            TaskOutcome::Agentic { report } => Some(report.clone()),
                            _ => None,
                        };
                        return (i, task, StepOutcome::Folded(report));
                    }
                    // Announce the task BEFORE the (slow) model call so the UI shows the native
                    // pass is running immediately — not blank until the first turn returns.
                    sink.task_started(model, &task.id, i, n_agentic_tasks, &task.category, true);
                    let turn = make_native(model, &task);
                    let (sandbox, cfg) = match sandbox_for(&task) {
                        Ok(v) => v,
                        Err(e) => return (i, task, StepOutcome::Ran { result: Err(e), wall_ms: 0 }),
                    };
                    let (tx, mut rx) = unbounded_channel::<TrajectoryStep>();
                    // Forward native steps to the UI sink (tagged is_native) so the user can
                    // WATCH the native run in the Evaluator — not a throwaway drain that hides it.
                    let (s2, model2, task2) = (sink.clone(), model.clone(), task.id.clone());
                    let pump = tokio::spawn(async move {
                        while let Some(step) = rx.recv().await {
                            s2.agentic_turn(&model2, &task2, &step, true); // native pass
                        }
                    });
                    let started = std::time::Instant::now();
                    let result = run_agentic_for(&turn, &task, model, &sandbox, cfg, cancel, concurrency, &tx).await;
                    drop(tx);
                    let _ = pump.await;
                    (i, task, StepOutcome::Ran { result, wall_ms: started.elapsed().as_millis() as u64 })
                }
            })
            .buffered(concurrency.max(1));

        while let Some((i, task, outcome)) = compute.next().await {
            let _ = i;
            match outcome {
                // Mirrors the old driving loop's `break` on cancel: stop committing further
                // tasks (already in-flight compute for later indices is dropped, not committed).
                StepOutcome::Cancelled => break,
                StepOutcome::Folded(Some(report)) => reports.push(report),
                StepOutcome::Folded(None) => {}
                StepOutcome::Ran { result: Ok(report), wall_ms } => {
                    let report = stamp_task_meta(report, &task).with_wall_ms(wall_ms);
                    let outcome = TaskOutcome::Agentic { report: report.clone() };
                    record(&CompletedUnit {
                        model: col.model.clone(),
                        task_id: task.id.clone(),
                        category: task.category.clone(),
                        outcome: outcome.clone(),
                        is_native: true,
                    });
                    // Stream this task's NATIVE result so the UI fills its native column as each
                    // task finishes — progressive, not only at batch-complete.
                    sink.task_done(&col.model, &task.id, &outcome, true);
                    reports.push(report);
                }
                // Every run of this task errored — a backend `Err`, NOT a scored failure (a
                // turn timeout is already scored). Count it visibly and classify the cause so
                // a host/infra crash is never read as model incapability. The dropped task is
                // why the native denominator silently shrank before this fix.
                StepOutcome::Ran { result: Err(e), .. } => {
                    let msg = e.to_string();
                    errored += 1;
                    error_class = merge_error_class(error_class, classify_native_error(&msg));
                    // Stream the per-task native ERROR too (the prompt pass does on its Err arm),
                    // so the Simulator's Tool-Calling cell shows "Error" for this task, not a
                    // stale "—" until the batch finishes.
                    sink.task_done(&col.model, &task.id, &TaskOutcome::Error { oom: is_oom_message(&msg), message: msg }, true);
                }
            }
        }
        drop(compute);
        // Emit the column when ANYTHING ran OR errored — an all-errored native pass now
        // surfaces "0 scored, N errored" instead of vanishing to `None`. `agg_agentic(&[])`
        // is empty-safe (total_runs 0), and `inputs.rs` filters native on `total_runs > 0`,
        // so an all-errored column never pollutes the verdict — it's pure visibility.
        if !reports.is_empty() || errored > 0 {
            let mut agg = agg_agentic(&reports, true);
            agg.tasks_errored = errored;
            agg.native_error_class = error_class;
            col.agentic_native_fc = Some(agg);
        }
        prev = Some(col.model.clone());
    }
    Ok(())
}

/// Does this backend+model support a NATIVE tool-calling API? Every remaining
/// backend serves the OpenAI tool wire — llama.cpp launched with `--jinja` applies
/// the model's embedded tool grammar, as do the remote vLLM servers — so all
/// are treated as capable. A template lacking tool support simply yields no
/// `tool_calls`, which the harness labels honestly rather than pre-judging here.
/// Kept as a function (not a constant) so a future backend with a real capability
/// probe has one place to hook in, and so both the GUI batch command and the
/// headless CLI keep dispatching through the same seam.
pub(crate) async fn probe_native_tools(backend: BackendKind, _endpoint_url: &str, _model: &str) -> bool {
    match backend {
        BackendKind::LlamaCpp | BackendKind::VLlm => true,
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;

use super::*;
use crate::errors::AppResult;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::model_turn::{BackendTurn, ModelTurn, Progress};
use crate::inference::eval::agentic::sandbox::{EndStateRule, TaskCheckpoint};
use crate::inference::eval::agentic::spec::AgenticSpec;
use crate::inference::eval::toolcall::matrix::ModelTarget;
use crate::inference::eval::toolcall::tasks::{Call, Expected, ToolSchema, ToolTask};
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// A backend-free model that returns one canned reply every turn.
struct ScriptedModel {
    reply: String,
}

impl ModelTurn for ScriptedModel {
    async fn run(&self, _spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        Ok((self.reply.clone(), GenerateStats { eval_count: Some(5), prompt_eval_count: Some(3), ..Default::default() }))
    }
}

fn make_turn(_t: &ModelTarget) -> ScriptedModel {
    ScriptedModel { reply: r#"{"name":"ping","args":{}}"#.into() }
}

#[derive(Default)]
struct CountingSink {
    started: Mutex<Vec<(String, String)>>,
    done: Mutex<u32>,
    native_done: Mutex<u32>,
    turns: Mutex<u32>,
    native_turns: Mutex<u32>,
}

impl BatchSink for CountingSink {
    fn task_started(&self, model: &str, task_id: &str, _i: usize, _total: usize, _cat: &str, _is_native: bool) {
        self.started.lock().unwrap().push((model.into(), task_id.into()));
    }
    fn agentic_turn(&self, _m: &str, _t: &str, _step: &TrajectoryStep, is_native: bool) {
        *self.turns.lock().unwrap() += 1;
        if is_native {
            *self.native_turns.lock().unwrap() += 1;
        }
    }
    fn task_done(&self, _m: &str, _t: &str, _o: &TaskOutcome, is_native: bool) {
        *self.done.lock().unwrap() += 1;
        if is_native {
            *self.native_done.lock().unwrap() += 1;
        }
    }
}

/// Diagnostic sink for the live truncation proof: records each step's kind + a raw-output
/// snippet so a failing proof shows WHAT the model did (emitted nothing? updated the chart?
/// called disclose?), instead of a silent all-zero attribution.
#[derive(Default)]
struct DiagSink {
    steps: Mutex<Vec<(String, String)>>,
    injections: Mutex<Vec<String>>,
}
impl BatchSink for DiagSink {
    fn task_started(&self, _m: &str, _t: &str, _i: usize, _total: usize, _cat: &str, _n: bool) {}
    fn agentic_turn(&self, _m: &str, _t: &str, step: &TrajectoryStep, _n: bool) {
        let snippet: String = step.raw_output.chars().take(160).collect::<String>().replace('\n', " ");
        self.steps.lock().unwrap().push((format!("{:?}", step.kind), snippet));
        if let Some(inj) = &step.injection {
            self.injections.lock().unwrap().push(inj.chars().take(220).collect());
        }
    }
    fn task_done(&self, _m: &str, _t: &str, _o: &TaskOutcome, _n: bool) {}
}

fn target(model: &str) -> ModelTarget {
    ModelTarget { model: model.into(), backend: BackendKind::Ollama, is_thinking: false }
}

fn tool(name: &str) -> ToolSchema {
    ToolSchema { name: name.into(), description: "d".into(), parameters: json!({ "type": "object", "properties": {} }) }
}

fn single_task(id: &str) -> ToolTask {
    ToolTask {
        id: id.into(),
        category: "single".into(),
        prompt: "p".into(),
        tools: vec![tool("ping")],
        expected: Expected::Call(Call { name: "ping".into(), args: json!({}) }),
        agentic: None,
    }
}

fn agentic_task(id: &str, k: u32) -> ToolTask {
    ToolTask {
        id: id.into(),
        category: "agentic".into(),
        prompt: "p".into(),
        tools: vec![tool("ping")],
        expected: Default::default(),
        agentic: Some(AgenticSpec {
            mocks: vec![],
            mcp: None,
            end_state: EndStateRule::RequireSequence(vec![TaskCheckpoint { tool: "ping".into(), args: json!({}) }]),
            environment: Default::default(),
            tier: Default::default(),
            axes: None,
            k: Some(k),
            max_steps: Some(4),
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

#[tokio::test]
async fn sequential_two_models_five_tasks_emits_ten_done_and_one_report() {
    let targets = vec![target("m1"), target("m2")];
    let tasks: Vec<ToolTask> = (0..5).map(|i| single_task(&format!("t{i}"))).collect();
    let sink = Arc::new(CountingSink::default());

    let report = run_batch("c", &targets, &tasks, CancellationToken::new(), sink.clone(), make_turn).await.unwrap();

    assert_eq!(*sink.done.lock().unwrap(), 10); // 2 models × 5 tasks
    assert_eq!(report.columns.len(), 2);

    // Strictly sequential: every m1 task completes before any m2 task starts.
    let started = sink.started.lock().unwrap();
    assert_eq!(started.len(), 10);
    assert!(started[..5].iter().all(|(m, _)| m == "m1"));
    assert!(started[5..].iter().all(|(m, _)| m == "m2"));
}

#[tokio::test]
async fn mixed_collection_streams_agentic_turns_and_aggregates_both() {
    let targets = vec![target("m1")];
    let tasks = vec![single_task("s1"), agentic_task("a1", 3)];
    let sink = Arc::new(CountingSink::default());

    let report = run_batch("c", &targets, &tasks, CancellationToken::new(), sink.clone(), make_turn).await.unwrap();

    assert_eq!(*sink.done.lock().unwrap(), 2);
    assert!(*sink.turns.lock().unwrap() >= 3, "expected ≥1 trajectory step per agentic run (k=3)");

    let col = &report.columns[0];
    assert!(col.toolcall.is_some(), "single-turn report present");
    let agg = col.agentic.as_ref().expect("agentic aggregate present");
    assert_eq!(agg.passes, 3);
    assert_eq!(agg.total_runs, 3);
}

#[tokio::test]
async fn batch_summaries_map_per_model_toolcall_and_agentic_metrics() {
    let targets = vec![target("m1")];
    let tasks = vec![single_task("s1"), agentic_task("a1", 4)];
    let sink = Arc::new(CountingSink::default());
    let report = run_batch("c", &targets, &tasks, CancellationToken::new(), sink, make_turn).await.unwrap();

    let sums = batch_summaries(&report, "2026-06-03T00:00:00Z");
    assert_eq!(sums.len(), 1);
    assert_eq!(sums[0].model, "m1");
    assert_eq!(sums[0].pass_k, Some(1.0)); // agentic a1 passed all 4 runs
    assert!(sums[0].composite.is_some()); // single-turn s1 contributes a composite
}

#[tokio::test]
async fn native_fc_pass_aggregates_into_the_column_for_supported_models_only() {
    let targets = vec![target("m1"), target("m2")];
    let tasks = vec![agentic_task("a1", 3)];
    let sink = Arc::new(CountingSink::default());
    let mut report = run_batch("c", &targets, &tasks, CancellationToken::new(), sink.clone(), make_turn).await.unwrap();

    // Only m1 reports the `tools` capability; m2 doesn't → stays N/A.
    let supported: std::collections::HashSet<String> = ["m1".to_string()].into_iter().collect();
    run_native_fc_pass(
        &mut report,
        &tasks,
        &supported,
        CancellationToken::new(),
        |_model, _task| ScriptedModel { reply: r#"{"name":"ping","args":{}}"#.into() },
        &[],
        &|_| {},
        &NoVramGate,
        sink.clone(),
    )
    .await
    .unwrap();

    let m1 = report.columns.iter().find(|c| c.model == "m1").unwrap();
    let m2 = report.columns.iter().find(|c| c.model == "m2").unwrap();
    assert_eq!(m1.agentic_native_fc.as_ref().unwrap().passes, 3); // native ping passes all 3 runs
    assert!(m2.agentic_native_fc.is_none()); // unsupported → never a fabricated native score
    // The native pass STREAMS its trajectory to the sink (tagged native) — not the old
    // throwaway drain — so the UI can show the native run.
    assert!(*sink.native_turns.lock().unwrap() > 0, "native steps must stream to the sink");
    // ...and it streams a per-task native OUTCOME (tagged native) so the UI fills its native
    // column progressively, as each native task finishes.
    assert_eq!(*sink.native_done.lock().unwrap(), 1, "native task_done must fire (tagged native) for the one supported model");
}

/// A native turn that ALWAYS errors with a fixed message (so every run errors → the task
/// produces no scored report), or pings to success when `err` is `None`.
struct NativeErrModel {
    err: Option<String>,
}
impl ModelTurn for NativeErrModel {
    async fn run(&self, _s: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        match &self.err {
            Some(m) => Err(crate::errors::AppError::Inference(m.clone())),
            None => Ok((r#"{"name":"ping","args":{}}"#.into(), GenerateStats { eval_count: Some(5), ..Default::default() })),
        }
    }
}

#[tokio::test]
async fn native_errored_tasks_are_counted_and_labeled_not_silently_dropped() {
    // Three native tasks: one scores, two error (every run) — one host/infra (5xx), one
    // schema-rejection (4xx). Before the fix the two errored tasks vanished from the
    // aggregate and the denominator silently shrank 3→1. Guard VISIBILITY, not arithmetic:
    // the scored denominator is honest AND the errored count + class are carried.
    let targets = vec![target("m1")];
    let tasks = vec![agentic_task("a_ok", 2), agentic_task("a_infra", 2), agentic_task("a_schema", 2)];
    let sink = Arc::new(CountingSink::default());
    let mut report = run_batch("c", &targets, &tasks, CancellationToken::new(), sink.clone(), make_turn).await.unwrap();

    let supported: std::collections::HashSet<String> = ["m1".to_string()].into_iter().collect();
    run_native_fc_pass(
        &mut report,
        &tasks,
        &supported,
        CancellationToken::new(),
        |_model, task| {
            let err = match task.id.as_str() {
                "a_infra" => Some("chat HTTP 500: ollama out of memory".to_string()),
                "a_schema" => Some("chat HTTP 400: tools not supported".to_string()),
                _ => None,
            };
            NativeErrModel { err }
        },
        &[],
        &|_| {},
        &NoVramGate,
        sink.clone(),
    )
    .await
    .unwrap();

    let agg = report.columns[0].agentic_native_fc.as_ref().expect("native column emitted despite errors");
    assert_eq!(agg.tasks_passed, 1); // only a_ok scored a pass
    assert_eq!(agg.tasks_total, 1); // scored denominator is NOT inflated by infra (pass_k stays honest)
    assert_eq!(agg.tasks_errored, 2); // ...but the 2 dropped tasks are now VISIBLE, not silent
    // Both an infra and a schema error occurred → Mixed (proves the labels did NOT collapse).
    assert_eq!(agg.native_error_class, NativeErrorClass::Mixed);
}

#[test]
fn native_error_classification_keeps_host_and_schema_labels_distinct() {
    // A 4xx is the native path rejecting the tool schema (a real "can't run native");
    // everything else is infra/host and must NEVER read as model incapability.
    assert_eq!(classify_native_error("chat HTTP 400: tools not supported"), NativeErrorClass::SchemaRejected);
    assert_eq!(classify_native_error("chat HTTP 500: ollama out of memory"), NativeErrorClass::InfraHost);
    assert_eq!(classify_native_error("connect to Ollama: connection refused"), NativeErrorClass::InfraHost);
    // The two never silently collapse — mixing distinct classes yields Mixed, not a merge.
    assert_eq!(merge_error_class(NativeErrorClass::InfraHost, NativeErrorClass::SchemaRejected), NativeErrorClass::Mixed);
    assert_eq!(merge_error_class(NativeErrorClass::None, NativeErrorClass::SchemaRejected), NativeErrorClass::SchemaRejected);
    assert_eq!(merge_error_class(NativeErrorClass::InfraHost, NativeErrorClass::InfraHost), NativeErrorClass::InfraHost);
}

#[test]
fn agg_agentic_sums_failure_breakdown_not_just_top_error() {
    use crate::inference::eval::agentic::scoring::report::{AgenticReport, FailureKind, RunOutcome};
    // Task A loops once; task B hallucinates nine times. `top_error` is Hallucinated
    // (9 > 1), but a `forbid_infinite_loop` verdict must still see the single loop —
    // the gap this aggregate closes.
    let a = AgenticReport::from_outcomes(&[RunOutcome::failure(4, 10, FailureKind::InfiniteLoop)]);
    let b_outcomes: Vec<RunOutcome> =
        (0..9).map(|_| RunOutcome::failure(2, 5, FailureKind::Hallucinated)).collect();
    let b = AgenticReport::from_outcomes(&b_outcomes);

    let agg = agg_agentic(&[a, b], false);

    assert_eq!(agg.top_error, TopError::Hallucinated); // headline still the majority mode
    assert_eq!(agg.failures.infinite_loop_hits, 1); // …but the loop is NOT hidden
    assert_eq!(agg.failures.hallucinated_completions, 9);
}

use crate::inference::eval::agentic::scoring::report::{AgenticReport, FailureKind, RunOutcome};
use std::sync::Mutex as StdMutex;

/// A gate that always fails — to prove the run halts (assert-and-fail) rather than
/// load the next model onto dirty VRAM.
struct FailingGate;
impl VramGate for FailingGate {
    async fn unload(&self, _model: &str) -> AppResult<()> {
        Err(crate::errors::AppError::Inference("VRAM stuck".into()))
    }
}

fn completed_agentic(model: &str, task: &str, passes: u32) -> CompletedUnit {
    let outcomes: Vec<RunOutcome> = (0..passes).map(|_| RunOutcome::success(2, 50)).collect();
    CompletedUnit {
        model: model.into(),
        task_id: task.into(),
        category: "agentic".into(),
        outcome: TaskOutcome::Agentic { report: AgenticReport::from_outcomes(&outcomes) },
        is_native: false,
    }
}

#[tokio::test]
async fn resume_folds_a_completed_unit_without_re_running_or_re_emitting() {
    let targets = vec![target("m1")];
    let tasks = vec![agentic_task("a1", 1)];
    let sink = Arc::new(CountingSink::default());
    // Prior says a1 already passed (1/1). The live turn would FAIL it (wrong tool) —
    // so if the report shows a pass, the unit was folded, not re-run.
    let prior = vec![completed_agentic("m1", "a1", 1)];
    let failing_turn = |_t: &ModelTarget| ScriptedModel { reply: r#"{"name":"wrong","args":{}}"#.into() };

    let report = run_batch_resumable(
        "c", &targets, &tasks, CancellationToken::new(), sink.clone(), failing_turn,
        &prior, &|_| {}, &NoVramGate,
    )
    .await
    .unwrap();

    assert_eq!(report.columns[0].agentic.as_ref().unwrap().passes, 1); // the prior success, not a re-run failure
    assert_eq!(*sink.done.lock().unwrap(), 0); // folded silently — no task_done replay (no IPC flood)
}

#[test]
fn fold_report_rebuilds_a_partial_from_completed_prompt_and_native_units() {
    // The bulk-rehydration core: on resume, completed units paint the Matrix in
    // one report — prompt units → `agentic`, native units → `agentic_native_fc`.
    let targets = vec![target("m1")];
    let tasks = vec![agentic_task("a1", 3)];
    let mut native = completed_agentic("m1", "a1", 1);
    native.is_native = true; // 1/1 native
    let prior = vec![completed_agentic("m1", "a1", 2), native]; // prompt 2/2 + native 1/1

    let report = fold_report("c", &targets, &tasks, &prior);

    let col = &report.columns[0];
    assert_eq!(col.agentic.as_ref().unwrap().passes, 2); // prompt folded
    assert_eq!(col.agentic_native_fc.as_ref().unwrap().passes, 1); // native folded (first-class)
}

#[tokio::test]
async fn vram_gate_error_halts_the_run_with_records_already_appended() {
    let targets = vec![target("m1"), target("m2")]; // both Ollama → a model switch
    let tasks = vec![agentic_task("a1", 1)];
    let sink = Arc::new(CountingSink::default());
    let recorded: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let rec = recorded.clone();
    let record = move |u: &CompletedUnit| rec.lock().unwrap().push(format!("{}/{}", u.model, u.task_id));

    let result = run_batch_resumable(
        "c", &targets, &tasks, CancellationToken::new(), sink, make_turn,
        &[], &record, &FailingGate,
    )
    .await;

    assert!(result.is_err()); // the stuck-VRAM gate halts — never loads m2 onto dirty VRAM
    // m1's unit was appended BEFORE the halt at the m1→m2 switch (no lost work).
    assert_eq!(*recorded.lock().unwrap(), vec!["m1/a1".to_string()]);
}

#[tokio::test]
async fn cancellation_stops_the_queue_early() {
    let targets = vec![target("m1")];
    let tasks: Vec<ToolTask> = (0..5).map(|i| single_task(&format!("t{i}"))).collect();
    let sink = Arc::new(CountingSink::default());
    let cancel = CancellationToken::new();
    cancel.cancel(); // pre-cancelled → no task should run

    let report = run_batch("c", &targets, &tasks, cancel, sink.clone(), make_turn).await.unwrap();

    assert_eq!(*sink.done.lock().unwrap(), 0);
    assert_eq!(report.columns.len(), 1); // the column is still emitted (empty)
}

// ── Pass^k aggregation: a task is credited only when ALL k runs pass (spec §3.3) ──

/// One task's report: `p` of `k` runs reached the end state, the rest hallucinated.
fn task_report(p: u32, k: u32) -> AgenticReport {
    let outcomes: Vec<RunOutcome> = (0..k)
        .map(|i| if i < p { RunOutcome::success(2, 10) } else { RunOutcome::failure(2, 10, FailureKind::Hallucinated) })
        .collect();
    AgenticReport::from_outcomes(&outcomes)
}

#[test]
fn a_budget_truncated_task_is_not_credited_as_a_strict_pass_k() {
    // Truncated at 1 of 16, and that single run passed: `passes == total_runs` (1 == 1)
    // would otherwise credit it as a full pass^16. The truncation flag must veto that —
    // we never observed the other 15 runs, so the all-k guarantee is unproven.
    let agg = agg_agentic(&[task_report(1, 1).with_truncation(16)], false);
    assert_eq!(agg.tasks_passed, 0, "a truncated batch can't claim the all-k guarantee");
    assert_eq!(agg.tasks_total, 1);
    // The observed run still feeds the secondary per-run rate honestly.
    assert_eq!(agg.passes, 1);
    assert_eq!(agg.total_runs, 1);
}

#[test]
fn pass_k_credits_a_task_only_when_all_k_runs_pass() {
    // Two flaky tasks (3/5 and 4/5): pass@k would read 7/10 = 0.7, but neither task
    // passed ALL k, so strict Pass^k is 0 — the whole point of the metric.
    let agg = agg_agentic(&[task_report(3, 5), task_report(4, 5)], false);
    assert_eq!(agg.tasks_passed, 0);
    assert_eq!(agg.tasks_total, 2);
    assert_eq!(agg.pass_k(), Some(0.0));
    // Run-level sums survive as the secondary per-run rate (pass@k 0.7).
    assert_eq!(agg.passes, 7);
    assert_eq!(agg.total_runs, 10);
}

#[test]
fn agg_buckets_strict_pass_k_by_tier() {
    use crate::inference::eval::agentic::spec::Tier;
    // Two Hard tasks (one all-k pass, one flaky) and one Easy task (all-k pass).
    let reports = vec![
        task_report(5, 5).with_tier(Tier::Easy),
        task_report(16, 16).with_tier(Tier::Hard),
        task_report(3, 5).with_tier(Tier::Hard),
    ];
    let agg = agg_agentic(&reports, false);

    let easy = agg.by_tier.iter().find(|s| s.tier == Tier::Easy).unwrap();
    assert_eq!((easy.tasks_passed, easy.tasks_total), (1, 1));
    assert_eq!(easy.pass_k(), Some(1.0));

    let hard = agg.by_tier.iter().find(|s| s.tier == Tier::Hard).unwrap();
    assert_eq!((hard.tasks_passed, hard.tasks_total), (1, 2)); // only the all-k task counts
    assert_eq!(hard.pass_k(), Some(0.5));

    // Buckets are sorted ascending by tier (the readiness gate walks them).
    assert!(agg.by_tier.windows(2).all(|w| w[0].tier <= w[1].tier));
    // Medium had no task → it's simply absent, never a fabricated 0.
    assert!(!agg.by_tier.iter().any(|s| s.tier == Tier::Medium));
}

#[test]
fn agg_buckets_per_tier_avg_steps_and_failures() {
    use crate::inference::eval::agentic::spec::Tier;
    // One clean Easy task, one clean Hard task, one flaky Hard task (2 of its 5 runs
    // hallucinate). Every run takes 2 steps (the task_report helper).
    let reports = vec![
        task_report(5, 5).with_tier(Tier::Easy),
        task_report(16, 16).with_tier(Tier::Hard),
        task_report(3, 5).with_tier(Tier::Hard),
    ];
    let agg = agg_agentic(&reports, false);

    let easy = agg.by_tier.iter().find(|s| s.tier == Tier::Easy).unwrap();
    let hard = agg.by_tier.iter().find(|s| s.tier == Tier::Hard).unwrap();

    // Per-tier avg steps = mean of that tier's reports' avg_steps (every run took 2 steps).
    assert_eq!(easy.avg_steps, Some(2.0));
    assert_eq!(hard.avg_steps, Some(2.0));

    // Failures are bucketed per tier, NOT smeared across tiers: the 2 hallucinated runs
    // belong to the Hard bucket only; Easy carries none.
    assert_eq!(hard.failures.hallucinated_completions, 2);
    assert_eq!(easy.failures.hallucinated_completions, 0);

    // The overall aggregate still sums failures across all tiers (unchanged behavior).
    assert_eq!(agg.failures.hallucinated_completions, 2);
}

#[test]
fn tier_stat_deserializes_a_pre_9b_payload_with_defaulted_per_tier_fields() {
    use crate::inference::eval::agentic::spec::Tier;
    // A TierStat written before Phase 9B carries no `avg_steps`/`failures` — they must
    // default (None / zeroed), never fail the parse.
    let s: TierStat =
        serde_json::from_value(serde_json::json!({ "tier": "hard", "tasks_passed": 1, "tasks_total": 2 })).unwrap();
    assert_eq!(s.tier, Tier::Hard);
    assert_eq!(s.avg_steps, None);
    assert_eq!(s.failures, FailureTracker::default());
}

#[test]
fn pass_k_is_the_fraction_of_fully_passing_tasks() {
    // One task clean (5/5), one fully failing (0/5): one of two tasks credited → 0.5.
    let agg = agg_agentic(&[task_report(5, 5), task_report(0, 5)], false);
    assert_eq!(agg.tasks_passed, 1);
    assert_eq!(agg.tasks_total, 2);
    assert_eq!(agg.pass_k(), Some(0.5));
    // Both tasks clean → 1.0.
    assert_eq!(agg_agentic(&[task_report(5, 5), task_report(5, 5)], false).pass_k(), Some(1.0));
}

/// A model that records each `warm_up` and `run` event with its model name, proving
/// the batch warms a model resident BEFORE running any of its scored tasks.
struct WarmTrackModel {
    log: Arc<Mutex<Vec<String>>>,
    model: String,
}

impl ModelTurn for WarmTrackModel {
    async fn run(&self, _s: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        self.log.lock().unwrap().push(format!("run:{}", self.model));
        Ok((r#"{"name":"ping","args":{}}"#.into(), GenerateStats { eval_count: Some(5), ..Default::default() }))
    }
    async fn warm_up(&self) -> AppResult<()> {
        self.log.lock().unwrap().push(format!("warm:{}", self.model));
        Ok(())
    }
}

#[tokio::test]
async fn warms_up_each_model_once_before_its_first_scored_task() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let l2 = log.clone();
    let make = move |t: &ModelTarget| WarmTrackModel { log: l2.clone(), model: t.model.clone() };
    let targets = vec![target("m1"), target("m2")];
    let tasks: Vec<ToolTask> = (0..3).map(|i| single_task(&format!("t{i}"))).collect();
    let sink = Arc::new(CountingSink::default());
    run_batch("c", &targets, &tasks, CancellationToken::new(), sink, make).await.unwrap();

    let ev = log.lock().unwrap().clone();
    // Warmed exactly once per model.
    assert_eq!(ev.iter().filter(|e| e.as_str() == "warm:m1").count(), 1);
    assert_eq!(ev.iter().filter(|e| e.as_str() == "warm:m2").count(), 1);
    // Each model's warm precedes every one of its runs (cold-load isn't charged to t0).
    for m in ["m1", "m2"] {
        let warm = ev.iter().position(|e| *e == format!("warm:{m}")).unwrap();
        let first_run = ev.iter().position(|e| *e == format!("run:{m}")).unwrap();
        assert!(warm < first_run, "{m}: warm_up must precede the first scored run");
    }
}

#[test]
fn ollama_version_makes_a_native_garble_diagnosable_on_the_report() {
    // Closes the Gap-C loop: a garbled native run (ForeignDialect) AND the Ollama version
    // coexist on the report, so a native tool-calling regression on a version bump reads as
    // "garbled at Ollama vX" — diagnosable, never a silent zero. Survives the serde round-trip
    // the saved/published report uses.
    use crate::inference::eval::agentic::scoring::report::{FailureTracker, TopError};
    let garbled = AggAgentic {
        tasks_passed: 0,
        tasks_total: 1,
        passes: 0,
        total_runs: 1,
        avg_steps: None,
        avg_output_tokens_success: None,
        schema_resilience: None,
        top_error: TopError::ForeignDialect,
        failures: FailureTracker { foreign_dialect_calls: 1, ..Default::default() },
        by_tier: vec![],
        tasks_errored: 0,
        native_error_class: Default::default(),
        boundary: None,
        tokens_per_completed: None,
            diagnostic: None,
            native_structured_calls: None,
            native_salvaged_calls: None,
    };
    let report = BatchReport {
        collection_id: "c".into(),
        num_ctx: None,
        ollama_version: Some("0.11.10".into()),
        collection_hash: None,
        think_preset: None,
        columns: vec![BatchColumn {
            model: "qwen3".into(),
            backend: BackendKind::Ollama,
            toolcall: None,
            agentic: None,
            agentic_native_fc: Some(garbled),
            error: None,
            is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
            ..Default::default()
        }],
    };
    let round: BatchReport = serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    // Both signals present together → the regression is diagnosable.
    assert_eq!(round.ollama_version.as_deref(), Some("0.11.10"));
    let native = round.columns[0].agentic_native_fc.as_ref().unwrap();
    assert_eq!(native.top_error, TopError::ForeignDialect);
    assert_eq!(native.failures.foreign_dialect_calls, 1);
}

#[tokio::test]
#[ignore = "DIAG: replicate the app's native pass for gemma4 — probe + run_native_fc_pass + streaming"]
async fn live_diag_app_native_pass_for_gemma4() {
    use crate::inference::eval::agentic::model_turn::NativeToolTurn;
    use crate::inference::eval::agentic::sandbox::EndStateRule as ESR;
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    use crate::inference::eval::toolcall::prompt::TerminalGuidance;
    use crate::inference::ollama::ollama_show::probe_supports_tools;

    const GEMMA: &str = "gemma-4-12b-it-qat:q4_0";
    let endpoint = "http://localhost:11434";

    // 1) Probe — exactly what batch_cmd does to build the `supported` set.
    let supports = probe_supports_tools(endpoint, GEMMA).await;
    eprintln!("STEP probe_supports_tools({GEMMA}) = {supports}");
    let supported: std::collections::HashSet<String> =
        if supports { [GEMMA.to_string()].into_iter().collect() } else { Default::default() };

    // One fast reporter-tool task.
    let tasks: Vec<ToolTask> = load_v2_collection(v2_json("easy-coding").unwrap())
        .unwrap()
        .into_iter()
        .filter(|t| t.id == "es_co_run_failing_test")
        .collect();

    // A report with a gemma4 Ollama column (as the prompt pass would leave it).
    let mut report = BatchReport {
        collection_id: "easy-coding".into(),
        num_ctx: None,
        ollama_version: None,
        collection_hash: None,
        think_preset: None,
        columns: vec![BatchColumn {
            model: GEMMA.into(),
            backend: BackendKind::Ollama,
            toolcall: None,
            agentic: None,
            agentic_native_fc: None,
            error: None,
            is_thinking: false,
            cpu_offloaded: false,
            ctx_ceiling: None,
            ..Default::default()
        }],
    };
    let sink = Arc::new(CountingSink::default());
    run_native_fc_pass(
        &mut report,
        &tasks,
        &supported,
        CancellationToken::new(),
        |model, task| {
            let terminal = match task.agentic.as_ref().map(|s| &s.end_state) {
                Some(ESR::RequireAll(_)) | Some(ESR::RequireSequence(_)) => TerminalGuidance::MustUseTools,
                _ => TerminalGuidance::PlainTextOk,
            };
            NativeToolTurn { backend: BackendKind::Ollama, endpoint: endpoint.to_string(), model: model.to_string(), tools: task.tools.clone(), options: None, terminal, max_tokens: 256, is_thinking: false }
        },
        &[],
        &|_| {},
        &NoVramGate,
        sink.clone(),
    )
    .await
    .unwrap();

    let col = &report.columns[0];
    eprintln!("STEP agentic_native_fc = {:?}", col.agentic_native_fc);
    eprintln!("STEP native_turns streamed = {}", *sink.native_turns.lock().unwrap());
    eprintln!("(N/A in the matrix means agentic_native_fc is None or total_runs==0 → all errored)");
}

// ── Category K: LIVE boundary run against a real Ollama model (rule 6) ────────────
// Ignored by default (needs Ollama on :11434 + the model pulled). Run with:
//   cargo test --lib category_k_live_boundary_run -- --ignored --nocapture
// Set QM_LIVE_MODEL / QM_LIVE_COLLECTION to try another model / domain. Prints the
// real BoundaryReport so resistance / over-refusal / attribution / gate can be
// inspected against the trace.
#[tokio::test]
#[ignore = "live: requires a running Ollama server and a pulled model"]
async fn category_k_live_boundary_run() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder-7b-instruct:q4_k_m".into());
    let collection = std::env::var("QM_LIVE_COLLECTION").unwrap_or_else(|_| "boundary-coding".into());
    let tasks = load_v2_collection(v2_json(&collection).unwrap()).unwrap();

    let targets = vec![ModelTarget { model: model.clone(), backend: BackendKind::Ollama, is_thinking: false }];
    let sink = Arc::new(CountingSink::default());
    let make = move |t: &ModelTarget| BackendTurn {
        backend: BackendKind::Ollama,
        endpoint: "http://localhost:11434".into(),
        model: t.model.clone(),
        cancel: CancellationToken::new(),
        options: None,
        keep_alive: None,
        is_thinking: false,
        max_tokens: 512,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };

    let report = run_batch(&collection, &targets, &tasks, CancellationToken::new(), sink, make).await.unwrap();
    let agg = report.columns[0].agentic.as_ref().expect("agentic aggregate present");
    let b = agg.boundary.as_ref().expect("boundary report present for a Category-K collection");

    println!("\n=== Category K live: {model} on {collection} ===");
    println!("capability pass_k (separate axis): {:?}", agg.pass_k());
    println!("resistance:       {:?}  ({}/{} attack runs resisted)", b.resistance, b.resisted, b.attack_probes);
    println!("over-refusal:     {:?}  ({}/{} benign runs refused)", b.over_refusal_rate, b.over_refusals, b.benign_probes);
    println!("attribution:      model_followed={} guard_truncated={} unattributed={}", b.attribution.model_followed, b.attribution.guard_truncated, b.attribution.unattributed);
    println!("by_attack:        {:?}", b.by_attack);
    println!("gate:             {:?}", b.gate);
    println!("caveat:           {}", b.caveat);

    // Pipeline assertions (not a model-quality judgment): both arms ran and the metric exists.
    // `over_refusal_rate` may legitimately be None (a weak model whose benign runs were all
    // capability failures never reaches a decision) — that's the trichotomy working, not a bug.
    assert!(b.attack_probes > 0, "attack arm must have run");
    assert!(b.benign_probes > 0, "benign control arm must have run");
    assert!(b.resistance.is_some(), "resistance measured over the attack arm");
    // Over-refused + capability-failed can't exceed the benign runs (the rest proceeded) —
    // the trichotomy is a real partition, not double-counted.
    assert!(b.over_refusals + b.benign_capability_failures <= b.benign_probes);
    assert!(!b.caveat.is_empty(), "static-set caveat must be present");
}

// ── Category K: LIVE proof of GuardTruncatedByConfig (the differentiator) ─────────
// Runs the context-squeeze collection at a SMALL served window (num_ctx=4096) vs a
// LARGE one (num_ctx=16384) on the SAME model + SAME scenario. Both is_thinking=true so
// num_ctx == ctx_ceiling exactly (agentic_num_ctx). The ~24KB record saturates 4096 (the
// front-placed guard is evicted → a forbidden call is attributed to the config) but fits
// 16384 (the guard survives → the model owns any forbidden call). The FLIP, on identical
// content, is the end-to-end proof the config verdict tracks real window pressure.
//   cargo test --lib category_k_live_guard_truncation_proof -- --ignored --nocapture
#[tokio::test]
#[ignore = "live: requires a running Ollama server and a pulled model"]
async fn category_k_live_guard_truncation_proof() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;

    // Model choice matters: it must (a) follow the injected disclose directive and (b) still emit
    // a PARSEABLE tool call when the window is filled. qwen2.5-coder does both; a 1B model emits
    // the call as prose the parser can't read. The scenario's record is sized so the whole prompt
    // ≈ the 4096 window: it fills the small window (occupancy caps at num_ctx → the front guard is
    // evicted to make generation room, yet only the front is lost, so the call stays parseable),
    // while the 16384 window has headroom (guard retained). Bigger records over-evict and garble
    // the call (a real prompt-path observability limit — see docs).
    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder-7b-instruct:q4_k_m".into());
    let tasks = load_v2_collection(v2_json("boundary-context-squeeze").unwrap()).unwrap();

    // `ceiling` sets the served window directly (is_thinking=false, scenario max_steps=40 so
    // num_ctx = clamp(2048+40*384, 4096, ceiling) = ceiling): 4096 (fills → guard evicted) vs
    // 16384 (fits → guard retained). Same content, same model — only the window differs.
    async fn probe(model: &str, tasks: &[ToolTask], ceiling: u32) -> (u32, u32, u32) {
        let sink = Arc::new(DiagSink::default());
        let ceil = ceiling;
        let make = move |t: &ModelTarget| BackendTurn {
            backend: BackendKind::Ollama,
            endpoint: "http://localhost:11434".into(),
            model: t.model.clone(),
            cancel: CancellationToken::new(),
            options: None,
            keep_alive: None,
            is_thinking: false,
            max_tokens: 512,
            cpu_offloaded: false,
            ctx_ceiling: ceil,
            stop_cache: Default::default(),
        };
        let targets = vec![ModelTarget { model: model.into(), backend: BackendKind::Ollama, is_thinking: false }];
        let report = run_batch("boundary-context-squeeze", &targets, tasks, CancellationToken::new(), sink.clone(), make).await.unwrap();
        let a = report.columns[0].agentic.as_ref().and_then(|g| g.boundary.as_ref()).map(|b| b.attribution).unwrap_or_default();
        println!("  [ceiling={ceiling}] steps for {model}:");
        for (kind, snip) in sink.steps.lock().unwrap().iter().take(8) {
            println!("    {kind:<22} | {snip}");
        }
        (a.model_followed, a.guard_truncated, a.unattributed)
    }

    // SMALL window (num_ctx=4096): the record saturates it → front guard evicted.
    let (sm_model, sm_guard, sm_unattr) = probe(&model, &tasks, 4096).await;
    println!("\n=== GuardTruncation proof: {model} ===");
    println!("SMALL num_ctx=4096  → model_followed={sm_model} guard_truncated={sm_guard} unattributed={sm_unattr}");
    // LARGE window (num_ctx=16384): the same record fits → the guard survives.
    let (lg_model, lg_guard, lg_unattr) = probe(&model, &tasks, 16384).await;
    println!("LARGE num_ctx=16384 → model_followed={lg_model} guard_truncated={lg_guard} unattributed={lg_unattr}");

    // The proof: config attribution appears ONLY under window pressure, on identical content.
    assert!(sm_guard >= 1, "small window must attribute at least one forbidden call to GuardTruncatedByConfig (saturation evicts the guard)");
    assert_eq!(lg_guard, 0, "large window must NOT attribute to config — the guard fit, so a forbidden call is the model's");
}

// ── Feature A + B: LIVE payload-noise extraction + Tokens/Task (rule 6) ───────────
//   cargo test --lib noisy_extraction_live -- --ignored --nocapture
// Runs the noisy-extraction collection against a real Ollama model. Confirms the getter
// returns the messy envelope (`data`/`_meta`/pagination) the model must dig the field out
// of, and prints Effort vs Tokens/Task (T*) so the amortized cost is visible on a real run.
#[tokio::test]
#[ignore = "live: requires a running Ollama server and a pulled model"]
async fn noisy_extraction_live() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder-7b-instruct:q4_k_m".into());
    let tasks = load_v2_collection(v2_json("noisy-extraction").unwrap()).unwrap();
    let targets = vec![ModelTarget { model: model.clone(), backend: BackendKind::Ollama, is_thinking: false }];
    let sink = Arc::new(DiagSink::default());
    let make = move |t: &ModelTarget| BackendTurn {
        backend: BackendKind::Ollama,
        endpoint: "http://localhost:11434".into(),
        model: t.model.clone(),
        cancel: CancellationToken::new(),
        options: None,
        keep_alive: None,
        is_thinking: false,
        max_tokens: 512,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };

    let report = run_batch("noisy-extraction", &targets, &tasks, CancellationToken::new(), sink.clone(), make).await.unwrap();
    let agg = report.columns[0].agentic.as_ref().expect("agentic aggregate present");

    println!("\n=== noisy-extraction live: {model} ===");
    println!("pass_k:        {:?}", agg.pass_k());
    println!("Effort:        {:?} (mean output tokens over successes)", agg.avg_output_tokens_success);
    println!("Tokens/Task:   {:?} (T* — total tokens / completions, incl. failed-run waste)", agg.tokens_per_completed);
    println!("--- a tool result the model saw (should be a noisy envelope) ---");
    let injections = sink.injections.lock().unwrap();
    if let Some(first) = injections.first() {
        println!("  {first}");
    }
    println!("--- model steps (kind | raw) ---");
    for (kind, raw) in sink.steps.lock().unwrap().iter().take(10) {
        println!("  {kind:<22} | {raw}");
    }

    // The getter returned the noisy envelope (nested `data` + metadata), not the bare blob.
    assert!(
        injections.iter().any(|i| i.contains("\"data\":") && i.contains("_meta")),
        "a getter result must be wrapped in the payload-noise envelope",
    );
    // T* is a real amortized cost when anything completed, and never below Effort.
    if let (Some(tstar), Some(eff)) = (agg.tokens_per_completed, agg.avg_output_tokens_success) {
        assert!(tstar >= eff - 1e-9, "T* ({tstar}) must be >= Effort ({eff}) — the waste tax");
    }
}

// ── LIVE verification of "Schema Resilience = —" (the user's Tool-Calling row) ─────────
//
// Claim under test: the "—" in SCHEMA RESIL. for a model that passes cleanly is the honest
// "no schema errors occurred, nothing to recover from" — NOT a lost value. Proven by the
// causal link, observed live, not asserted from code: a `StepKind::SchemaError` is the ONLY
// thing that can move `schema_resilience` off `None` (a recovered error → Some(1.0), an
// unrecovered one → Some(<1)). So `schema_error_steps == 0  <=>  schema_resilience == None`.
//
// This sink watches the runner's real trajectory and counts SchemaError steps per arm, so
// the verification reads the LOGS (the emitted steps), not just the final number.
//
// Run: QM_LIVE_MODEL=qwen3.5:9b cargo test --lib schema_resil_live -- --ignored --nocapture

#[derive(Default)]
struct SchemaWatchSink {
    prompt_schema_errors: Mutex<u32>,
    native_schema_errors: Mutex<u32>,
    prompt_turns: Mutex<u32>,
    native_turns: Mutex<u32>,
}

impl BatchSink for SchemaWatchSink {
    fn task_started(&self, _m: &str, _t: &str, _i: usize, _total: usize, _cat: &str, _is_native: bool) {}
    fn agentic_turn(&self, _m: &str, _t: &str, step: &TrajectoryStep, is_native: bool) {
        let (turns, errs) = if is_native {
            (&self.native_turns, &self.native_schema_errors)
        } else {
            (&self.prompt_turns, &self.prompt_schema_errors)
        };
        *turns.lock().unwrap() += 1;
        if step.kind == crate::inference::eval::agentic::step::StepKind::SchemaError {
            *errs.lock().unwrap() += 1;
        }
    }
    fn task_done(&self, _m: &str, _t: &str, _o: &TaskOutcome, _is_native: bool) {}
}

#[tokio::test]
#[ignore = "live: requires a running Ollama server with the model pulled"]
async fn schema_resil_live_none_iff_zero_schema_errors() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    use crate::inference::eval::agentic::model_turn::NativeToolTurn;
    use crate::inference::eval::toolcall::prompt::TerminalGuidance;
    use crate::inference::generate::generate_options::GenerateOptions;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
    let collection = std::env::var("QM_LIVE_COLLECTION").unwrap_or_else(|_| "easy-coding".into());
    let tasks = load_v2_collection(v2_json(&collection).unwrap()).unwrap();
    let endpoint = "http://127.0.0.1:11434".to_string();

    let targets = vec![ModelTarget { model: model.clone(), backend: BackendKind::Ollama, is_thinking: false }];
    let sink = Arc::new(SchemaWatchSink::default());

    // Prompt pass (populates `.agentic`, method "Prompt-based").
    let ep = endpoint.clone();
    let make = move |t: &ModelTarget| BackendTurn {
        backend: BackendKind::Ollama,
        endpoint: ep.clone(),
        model: t.model.clone(),
        cancel: CancellationToken::new(),
        options: None,
        keep_alive: None,
        is_thinking: false,
        max_tokens: 512,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };
    let mut report = run_batch(&collection, &targets, &tasks, CancellationToken::new(), sink.clone(), make)
        .await
        .unwrap();

    // Native FC pass (populates `.agentic_native_fc`, method "Tool-Calling" — the USER's row).
    let supported: std::collections::HashSet<String> = [model.clone()].into_iter().collect();
    let ep2 = endpoint.clone();
    let make_native = move |m: &str, task: &ToolTask| NativeToolTurn {
        backend: BackendKind::Ollama,
        endpoint: ep2.clone(),
        model: m.to_string(),
        tools: task.tools.clone(),
        options: Some(GenerateOptions { temperature: Some(0.0), num_predict: Some(512), ..Default::default() }),
        terminal: TerminalGuidance::PlainTextOk,
        max_tokens: 512,
        is_thinking: false,
    };
    run_native_fc_pass(
        &mut report, &tasks, &supported, CancellationToken::new(),
        make_native, &[], &|_| {}, &NoVramGate, sink.clone(),
    )
    .await
    .unwrap();

    let col = &report.columns[0];
    let prompt = col.agentic.as_ref().expect("prompt aggregate");
    let native = col.agentic_native_fc.as_ref().expect("native aggregate (the Tool-Calling row)");
    let p_err = *sink.prompt_schema_errors.lock().unwrap();
    let n_err = *sink.native_schema_errors.lock().unwrap();

    println!("\n=== SCHEMA RESILIENCE, live: {model} on {collection} ===");
    println!("  PROMPT  pass_k={:?}  passes={}/{}  schema_resilience={:?}  SchemaError steps observed={}",
        prompt.pass_k(), prompt.passes, prompt.total_runs, prompt.schema_resilience, p_err);
    println!("  NATIVE  pass_k={:?}  passes={}/{}  schema_resilience={:?}  SchemaError steps observed={}   <- the user's Tool-Calling row",
        native.pass_k(), native.passes, native.total_runs, native.schema_resilience, n_err);
    println!("  → schema_resilience renders as '—' exactly when SchemaError steps == 0.");

    // THE CAUSAL LINK, verified live on both arms: the metric is None if and ONLY if the
    // runner emitted zero schema-error steps. A "—" is therefore proof the model never
    // emitted a malformed call — never a swallowed value.
    assert_eq!(
        native.schema_resilience.is_none(), n_err == 0,
        "native: schema_resilience None must mean exactly zero schema errors (got resil={:?}, errors={})",
        native.schema_resilience, n_err,
    );
    assert_eq!(
        prompt.schema_resilience.is_none(), p_err == 0,
        "prompt: schema_resilience None must mean exactly zero schema errors (got resil={:?}, errors={})",
        prompt.schema_resilience, p_err,
    );
}

/// OOM classification is deliberately narrow: the strings local backends actually emit
/// classify true; an ambiguous infra error (5xx, dropped connection) must NOT claim OOM.
#[test]
fn oom_classification_matches_real_backend_strings_and_nothing_else() {
    assert!(is_oom_message("llama runner process has terminated: CUDA error: out of memory"));
    assert!(is_oom_message("Metal: kIOGPUCommandBufferCallbackErrorOutOfMemory"));
    assert!(is_oom_message("Not enough memory to run this model."));
    assert!(!is_oom_message("HTTP 500 from /api/chat"));
    assert!(!is_oom_message("connection reset by peer"));
    assert!(!is_oom_message("request timed out"));
}

/// `wall_ms` is a batch-layer stamp: `from_outcomes` must never fabricate one, and the
/// builder carries the measured value onto the report (same pattern `with_tier` uses).
#[test]
fn wall_ms_is_stamped_by_the_builder_never_fabricated() {
    let report = AgenticReport::from_outcomes(&[]);
    assert_eq!(report.wall_ms, None, "no clock ran → no wall time, never a fabricated 0");
    assert_eq!(report.with_wall_ms(1234).wall_ms, Some(1234));
}

// Run: QM_LIVE_MODEL=qwen2.5:3b cargo test --lib turn_costs_live -- --ignored --nocapture
// Rule-6 live check for the Inspector Test-run link: drives ONE agentic task (k=1) against a
// real Ollama and inspects the ACTUAL per-turn cost fields the runner now keeps — the exact
// acceptance criteria from the plan: eval_ms/total_ms/output_tokens Some on every real turn,
// cache_n None on Ollama (issue #8008 — must render "Not available"), wall_ms stamped on the
// per-task report, and backend_rss(Ollama) measurable while the server runs.
#[tokio::test]
#[ignore = "live: requires a running Ollama server and a pulled model"]
async fn turn_costs_live() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;

    #[derive(Default)]
    struct CostSink {
        steps: Mutex<Vec<TrajectoryStep>>,
        outcomes: Mutex<Vec<TaskOutcome>>,
    }
    impl BatchSink for CostSink {
        fn task_started(&self, _m: &str, _t: &str, _i: usize, _total: usize, _c: &str, _n: bool) {}
        fn agentic_turn(&self, _m: &str, _t: &str, step: &TrajectoryStep, _n: bool) {
            self.steps.lock().unwrap().push(step.clone());
        }
        fn task_done(&self, _m: &str, _t: &str, o: &TaskOutcome, _n: bool) {
            self.outcomes.lock().unwrap().push(o.clone());
        }
    }

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5:3b".into());
    let mut tasks = load_v2_collection(v2_json("easy-coding").unwrap()).unwrap();
    // One task, one run, tight step cap — the check is about metric plumbing, not pass rates.
    tasks.retain(|t| t.agentic.is_some());
    tasks.truncate(1);
    if let Some(a) = tasks[0].agentic.as_mut() {
        a.k = Some(1);
        a.max_steps = Some(4);
    }
    let targets = vec![ModelTarget { model: model.clone(), backend: BackendKind::Ollama, is_thinking: false }];
    let sink = Arc::new(CostSink::default());
    let make = move |t: &ModelTarget| BackendTurn {
        backend: BackendKind::Ollama,
        endpoint: "http://localhost:11434".into(),
        model: t.model.clone(),
        cancel: CancellationToken::new(),
        options: None,
        keep_alive: None,
        is_thinking: false,
        max_tokens: 512,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };

    let _ = run_batch("easy-coding", &targets, &tasks, CancellationToken::new(), sink.clone(), make).await.unwrap();

    let steps = sink.steps.lock().unwrap().clone();
    assert!(!steps.is_empty(), "the run must stream at least one turn");
    println!("\n=== turn_costs_live: {model}, {} steps ===", steps.len());
    for s in &steps {
        println!(
            "  r{}s{} {:?}: prefill={:?}tok/{:?}ms decode={:?}ms total={:?}ms out={:?}tok cache_n={:?}",
            s.run_index, s.step_index, s.kind, s.prefill_tokens, s.prefill_ms, s.eval_ms, s.total_ms, s.output_tokens, s.cache_n
        );
    }
    // Every REAL model turn (one with a prefill) must now carry the decode/total/output
    // fields the runner used to discard — and cache_n must stay None on Ollama (honesty:
    // the UI renders "Not available", never a fabricated number).
    let real: Vec<_> = steps.iter().filter(|s| s.prefill_tokens.is_some()).collect();
    assert!(!real.is_empty(), "at least one turn must carry server stats");
    for s in &real {
        assert!(s.eval_ms.is_some(), "Ollama reports eval_duration — must not be dropped");
        assert!(s.total_ms.is_some(), "Ollama reports total_duration — must not be dropped");
        assert!(s.output_tokens.is_some(), "eval_count must be carried for every model");
        assert!(s.cache_n.is_none(), "Ollama reports no cache count (ollama#8008) — never fabricate one");
    }
    // The per-task report carries the measured whole-batch wall clock.
    let outcomes = sink.outcomes.lock().unwrap().clone();
    let wall = outcomes.iter().find_map(|o| match o {
        TaskOutcome::Agentic { report } => report.wall_ms,
        _ => None,
    });
    println!("  task wall_ms: {wall:?}");
    assert!(wall.is_some_and(|w| w > 0), "wall_ms must be stamped on the task report");
    // The sink-side RSS probe is measurable while the server runs (the Tauri sink stamps
    // this per step; here we prove the probe itself against the live process).
    let rss = crate::commands::system::process_memory::backend_rss(BackendKind::Ollama);
    println!("  ollama RSS now: {rss:?}");
    assert!(rss.is_some_and(|b| b > 0), "a running Ollama must yield a positive RSS sample");
}

// Run: cargo test --lib turn_costs_live_llama -- --ignored --nocapture
// The llama.cpp arm of the rule-6 live check: same one-task run against llama-server
// (:8081, --jinja). The cache-truth acceptance criterion INVERTS here: llama.cpp's
// `timings` reports `cache_n` on every turn, so real turns must carry Some — the same
// UI that says "Not available" on Ollama shows measured cache hits here.
#[tokio::test]
#[ignore = "live: requires llama-server on :8081 (--jinja) with a model loaded"]
async fn turn_costs_live_llama() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;

    #[derive(Default)]
    struct CostSink {
        steps: Mutex<Vec<TrajectoryStep>>,
    }
    impl BatchSink for CostSink {
        fn task_started(&self, _m: &str, _t: &str, _i: usize, _total: usize, _c: &str, _n: bool) {}
        fn agentic_turn(&self, _m: &str, _t: &str, step: &TrajectoryStep, _n: bool) {
            self.steps.lock().unwrap().push(step.clone());
        }
        fn task_done(&self, _m: &str, _t: &str, _o: &TaskOutcome, _n: bool) {}
    }

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder-7b-instruct_q4_k_m".into());
    let mut tasks = load_v2_collection(v2_json("easy-coding").unwrap()).unwrap();
    tasks.retain(|t| t.agentic.is_some());
    tasks.truncate(1);
    if let Some(a) = tasks[0].agentic.as_mut() {
        a.k = Some(1);
        a.max_steps = Some(4);
    }
    let targets = vec![ModelTarget { model: model.clone(), backend: BackendKind::LlamaCpp, is_thinking: false }];
    let sink = Arc::new(CostSink::default());
    let make = move |t: &ModelTarget| BackendTurn {
        backend: BackendKind::LlamaCpp,
        endpoint: "http://localhost:8081".into(),
        model: t.model.clone(),
        cancel: CancellationToken::new(),
        options: None,
        keep_alive: None,
        is_thinking: false,
        max_tokens: 512,
        cpu_offloaded: false,
        ctx_ceiling: 4096, // clamp to the launched -c
        stop_cache: Default::default(),
    };

    let _ = run_batch("easy-coding", &targets, &tasks, CancellationToken::new(), sink.clone(), make).await.unwrap();

    let steps = sink.steps.lock().unwrap().clone();
    assert!(!steps.is_empty(), "the run must stream at least one turn");
    println!("\n=== turn_costs_live_llama: {model}, {} steps ===", steps.len());
    for s in &steps {
        println!(
            "  r{}s{} {:?}: prefill={:?}tok/{:?}ms decode={:?}ms total={:?}ms out={:?}tok cache_n={:?}",
            s.run_index, s.step_index, s.kind, s.prefill_tokens, s.prefill_ms, s.eval_ms, s.total_ms, s.output_tokens, s.cache_n
        );
    }
    let real: Vec<_> = steps.iter().filter(|s| s.prefill_tokens.is_some() || s.cache_n.is_some()).collect();
    assert!(!real.is_empty(), "at least one turn must carry server timings");
    for s in &real {
        assert!(s.eval_ms.is_some(), "llama.cpp reports predicted_ms — must not be dropped");
        assert!(s.output_tokens.is_some(), "predicted_n must be carried");
        assert!(s.cache_n.is_some(), "llama.cpp reports cache_n on every turn — the measured cache tier");
    }
    // Turn 2+ of a multi-turn run reuses the transcript prefix: measured cache hits > 0.
    if let Some(later) = real.iter().find(|s| s.step_index > 0) {
        assert!(later.cache_n.unwrap() > 0, "a follow-up turn must reuse the prefix cache");
    }
    let rss = crate::commands::system::process_memory::backend_rss(BackendKind::LlamaCpp);
    println!("  llama-server RSS now: {rss:?}");
    assert!(rss.is_some_and(|b| b > 0), "a running llama-server must yield a positive RSS sample");
}

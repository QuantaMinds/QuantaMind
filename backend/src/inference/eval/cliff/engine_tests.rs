use super::*;
use crate::inference::eval::toolcall::tasks::{Call, Expected, ToolSchema, ToolTask};
use crate::inference::generate::generate_stats::GenerateStats;
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// A scripted model whose reported prompt tokens track the real prompt length
/// (so verify-and-adjust converges) and which emits the correct call only while
/// the context stays under `threshold` — a deterministic accuracy cliff.
struct CliffModel {
    threshold: u32,
    good: String,
}

impl ModelTurn for CliffModel {
    async fn run(&self, spec: &GenerateSpec) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let toks = (chars / 4) as u32;
        let text = if toks < self.threshold { self.good.clone() } else { "I cannot help with that.".to_string() };
        Ok((text, GenerateStats { prompt_eval_count: Some(toks), ..Default::default() }))
    }
}

fn task() -> ToolTask {
    ToolTask {
        id: "t1".into(),
        category: "single".into(),
        prompt: "Get the balance for account A-1.".into(),
        tools: vec![ToolSchema {
            name: "get_balance".into(),
            description: "Look up an account balance".into(),
            parameters: json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }),
        }],
        expected: Expected::Call(Call { name: "get_balance".into(), args: json!({ "id": "A-1" }) }),
        agentic: None,
    }
}

const GOOD: &str = r#"{"name":"get_balance","args":{"id":"A-1"}}"#;

fn source() -> CliffSource {
    CliffSource::Preset { preset: super::super::presets::CliffPreset::CorporatePolicy }
}

#[tokio::test]
async fn verify_and_adjust_lands_each_rung_within_five_percent_of_target() {
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() }; // never collapses
    let tasks = [task()];
    let ladder = [2000u32, 8000, 16000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    for p in report.points.iter() {
        let off = (p.verified_tokens as f64 - p.target_tokens as f64).abs() / p.target_tokens as f64;
        assert!(off <= 0.05, "rung {} verified {} is >5% off", p.target_tokens, p.verified_tokens);
        // The reported depth is the VERIFIED count, not the requested one.
        assert_ne!(p.verified_tokens, 0);
    }
}

#[tokio::test]
async fn detects_the_cliff_and_reports_the_last_passing_depth() {
    // Correct under ~5000 tokens, garbage above → collapses at the 8000 rung.
    let model = CliffModel { threshold: 5000, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000, 16000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();

    let collapse_at = report.points.iter().find(|p| p.target_tokens == 8000).unwrap().verified_tokens;
    let last_pass = report.points.iter().find(|p| p.target_tokens == 2000).unwrap().verified_tokens;
    assert_eq!(report.status, CliffStatus::Collapsed { depth: collapse_at });
    // cliff_tokens = the largest VERIFIED context that still passed across positions.
    assert_eq!(report.cliff_tokens, Some(last_pass));
}

#[tokio::test]
async fn early_stop_skips_the_slow_deep_rungs_once_a_cliff_is_found() {
    // Collapses past ~5000 tokens. The 16000/32000 rungs are the slowest, and once
    // the 8000 rung collapses they add nothing — they must NOT be probed.
    let model = CliffModel { threshold: 5000, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000, 16000, 32000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    let probed: Vec<u32> = report.points.iter().map(|p| p.target_tokens).collect();
    assert_eq!(probed, vec![0, 2000, 8000]); // stopped at the collapse; deep rungs skipped
    assert!(matches!(report.status, CliffStatus::Collapsed { .. }));
}

#[tokio::test]
async fn early_stop_on_a_broken_baseline_probes_no_padded_rung() {
    let model = CliffModel { threshold: 0, good: GOOD.into() }; // fails even unpadded
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000, 16000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    assert_eq!(report.points.len(), 1); // only the baseline — no expensive padded rung
    assert!(matches!(report.status, CliffStatus::Broken { .. }));
}

#[tokio::test]
async fn a_model_that_holds_throughout_reports_no_cliff() {
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    let deepest = report.points.last().unwrap().verified_tokens;
    assert_eq!(report.status, CliffStatus::NoCliff { tested: deepest });
    assert_eq!(report.cliff_tokens, Some(deepest));
}

#[tokio::test]
async fn a_broken_baseline_is_never_a_fabricated_cliff() {
    // Fails even unpadded → no baseline → Broken, never a cliff number.
    let model = CliffModel { threshold: 0, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    let base = report.points[0].verified_tokens;
    assert_eq!(report.status, CliffStatus::Broken { tested: base });
    assert_eq!(report.cliff_tokens, None);
}

#[tokio::test]
async fn a_broken_baseline_captures_the_raw_failing_output() {
    // The model refuses unpadded → Broken. The baseline rung must carry the system
    // prompt + raw completion so the UI's "View trace" shows WHY it failed, not a bare 0%.
    let model = CliffModel { threshold: 0, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 2000, 8000];
    let report = run_cliff(&model, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS).await.unwrap();
    let base = &report.points[0];
    assert!(matches!(report.status, CliffStatus::Broken { .. }));
    assert_eq!(base.trace.len(), 1, "one task → one trace entry");
    assert_eq!(base.trace[0].task_id, "t1");
    let out = &base.trace[0].outputs[0];
    assert!(out.output.contains("cannot help"), "raw refusal text is kept verbatim: {:?}", out.output);
    assert!(!out.passed, "the refusal is marked as a failure");
    // The unpadded baseline's input IS the bare instruction (no padding injected yet).
    assert!(out.prompt.contains("Get the balance"), "the input prompt is captured: {:?}", out.prompt);
}

#[tokio::test]
async fn every_rung_captures_a_trace_for_each_task_pass_or_fail() {
    // The trace is per-step evidence for EVERY task, not failure-only: a model that holds
    // throughout still records each rung's system prompt + outputs (all marked passed).
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let report = run_cliff(&model, "m", &tasks, &source(), &[0u32, 4000, 8000], &DEFAULT_DEPTHS).await.unwrap();
    assert!(matches!(report.status, CliffStatus::NoCliff { .. }));
    for p in &report.points {
        assert_eq!(p.trace.len(), 1, "rung {} should trace its one task", p.target_tokens);
        let t = &p.trace[0];
        assert!(!t.outputs.is_empty(), "rung {} captured no output", p.target_tokens);
        assert!(t.outputs.iter().all(|o| o.passed), "a holding model's outputs are all passes");
    }
    // The baseline sweeps one position; a padded rung sweeps all default needle depths.
    assert_eq!(report.points[0].trace[0].outputs.len(), 1, "baseline is a single position");
    assert_eq!(report.points[1].trace[0].outputs.len(), DEFAULT_DEPTHS.len(), "padded rungs sweep every needle position");
    // A padded rung's input carries the injected padding — far larger than the bare
    // baseline instruction — so "View trace" shows the context that was fed in.
    let baseline_len = report.points[0].trace[0].outputs[0].prompt.chars().count();
    let padded_len = report.points[1].trace[0].outputs[0].prompt.chars().count();
    assert!(padded_len > baseline_len, "padded input ({padded_len}) should exceed the bare instruction ({baseline_len})");
}

fn agentic_task(id: &str) -> ToolTask {
    // A real agentic task carries a PLACEHOLDER `expected: no_call`; its true criterion is
    // the multi-turn `agentic.end_state`, which the single-turn cliff never scores.
    let mut t = task();
    t.id = id.into();
    t.category = "agentic".into();
    t.expected = Expected::NoCall;
    t
}

#[tokio::test]
async fn agentic_tasks_score_on_json_wellformedness_not_abstention() {
    // The bug this replaces: a valid tool call was failed as a bad abstention → fake
    // Broken 0%. Now an agentic task PASSES a rung whenever the model emits a well-formed
    // call, so a model emitting clean JSON reads as no-cliff, not Broken.
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let report = run_cliff(&model, "m", &[agentic_task("multi-step")], &source(), &[0u32, 4000], &DEFAULT_DEPTHS).await.unwrap();
    assert_eq!(report.points[0].composite, Some(1.0), "a well-formed JSON call passes the structural check");
    assert!(matches!(report.status, CliffStatus::NoCliff { .. }));
    assert!(
        report.points[0].trace.iter().flat_map(|t| &t.outputs).all(|o| o.passed),
        "a structural pass is traced as passed",
    );
}

#[tokio::test]
async fn an_agentic_task_with_broken_json_is_a_structural_failure() {
    // Non-JSON output (no parseable call) IS a real cliff signal for an agentic task —
    // the model's tool-call FORMAT broke at this depth — so it scores 0% and is captured.
    let model = CliffModel { threshold: 0, good: GOOD.into() }; // always prose, never JSON
    let report = run_cliff(&model, "m", &[agentic_task("multi-step")], &source(), &[0u32, 4000], &DEFAULT_DEPTHS).await.unwrap();
    assert_eq!(report.points[0].composite, Some(0.0));
    assert!(matches!(report.status, CliffStatus::Broken { .. }));
    assert!(
        report.points[0].trace.iter().flat_map(|t| &t.outputs).any(|o| o.output.contains("cannot help") && !o.passed),
        "the broken (non-JSON) output is captured as a failed trace entry",
    );
}

/// A model that cancels the shared token the moment it's asked to generate — simulates
/// a user Stop landing mid-rung (the in-flight turn aborts and returns partial text).
struct CancelsMidRun {
    cancel: CancellationToken,
}
impl ModelTurn for CancelsMidRun {
    async fn run(&self, _spec: &GenerateSpec) -> AppResult<(String, GenerateStats)> {
        self.cancel.cancel();
        Ok((String::new(), GenerateStats { prompt_eval_count: None, ..Default::default() }))
    }
}

#[tokio::test]
async fn a_cancel_during_a_rung_aborts_before_emitting_that_rung() {
    // The bug this guards: a cancelled (or superseded) run emitting its half-generated
    // rung — which then pollutes the chart with garbage/empty outputs. The engine must
    // abort with an error and emit nothing once the token is cancelled.
    let cancel = CancellationToken::new();
    let model = CancelsMidRun { cancel: cancel.clone() };
    let mut emitted = 0usize;
    let result = run_cliff_with(
        &model,
        "m",
        &[task()],
        &source(),
        &[0u32, 4000],
        &DEFAULT_DEPTHS,
        &cancel,
        &mut |_, _, _| {
            emitted += 1;
        },
        &mut |_| {},
    )
    .await;
    assert!(result.is_err(), "a cancel mid-rung aborts with an error");
    assert_eq!(emitted, 0, "the half-generated rung is never emitted");
}

#[tokio::test]
async fn a_per_task_turn_factory_scores_identically_to_a_shared_turn() {
    // The native path drives the cliff through `run_cliff_with_factory` (a fresh turn built per
    // task); a factory returning the SAME scripted model must yield the same report as the
    // shared-turn `run_cliff`, proving the factory seam is behavior-preserving — only the turn
    // construction differs, never the padding/sweep/scoring/classification.
    let tasks = [task()];
    let ladder = [0u32, 4000, 8000];
    let shared = run_cliff(&CliffModel { threshold: 5000, good: GOOD.into() }, "m", &tasks, &source(), &ladder, &DEFAULT_DEPTHS)
        .await
        .unwrap();
    let make = |_: &ToolTask| CliffModel { threshold: 5000, good: GOOD.into() };
    let factory = run_cliff_with_factory(
        &make,
        "m",
        &tasks,
        &source(),
        &ladder,
        &DEFAULT_DEPTHS,
        &CancellationToken::new(),
        &mut |_, _, _| {},
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(shared.status, factory.status, "same classified status");
    assert_eq!(shared.cliff_tokens, factory.cliff_tokens, "same cliff depth");
    assert_eq!(shared.points.len(), factory.points.len(), "same rungs probed");
}

#[test]
fn build_ladder_spans_zero_to_max_across_steps() {
    assert_eq!(build_ladder(16000, 5), vec![0, 4000, 8000, 12000, 16000]);
    let l = build_ladder(10000, 4);
    assert_eq!(l.first(), Some(&0));
    assert_eq!(l.last(), Some(&10000));
    assert_eq!(l.len(), 4);
}

#[tokio::test]
async fn progress_callback_fires_once_per_rung() {
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 4000, 8000];
    let mut seen: Vec<(usize, usize)> = Vec::new();
    let report = run_cliff_with(
        &model,
        "m",
        &tasks,
        &source(),
        &ladder,
        &DEFAULT_DEPTHS,
        &CancellationToken::new(),
        &mut |done, total, _| {
            seen.push((done, total));
        },
        &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(seen, vec![(1, 3), (2, 3), (3, 3)]);
    assert_eq!(report.points.len(), 3);
}

#[tokio::test]
async fn step_callback_fires_per_task_with_rung_and_position_context() {
    // The fix for the "stuck on rung 2" report: the engine must emit a fine-grained step
    // after EVERY task generation, carrying rung + needle-position + task context, so the
    // UI can show movement during a slow padded rung instead of freezing between rungs.
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 4000]; // baseline (1 position) + one padded rung (3 positions)
    let mut steps: Vec<(usize, usize, u32, usize, usize, usize, usize)> = Vec::new();
    run_cliff_with(
        &model,
        "m",
        &tasks,
        &source(),
        &ladder,
        &DEFAULT_DEPTHS,
        &CancellationToken::new(),
        &mut |_, _, _| {},
        &mut |s| steps.push((s.rung, s.total_rungs, s.target_tokens, s.position, s.total_positions, s.task, s.total_tasks)),
    )
    .await
    .unwrap();
    // Baseline: rung 1/2, target 0, single position, one task.
    assert_eq!(steps.first(), Some(&(1, 2, 0, 1, 1, 1, 1)));
    // Padded rung: rung 2/2, target 4000, swept across all three needle positions.
    let padded: Vec<_> = steps.iter().filter(|s| s.0 == 2).collect();
    assert_eq!(padded.len(), DEFAULT_DEPTHS.len(), "one step per needle position for the single task");
    assert_eq!(padded[0], &(2, 2, 4000, 1, 3, 1, 1));
    assert_eq!(padded[2], &(2, 2, 4000, 3, 3, 1, 1));
}

#[tokio::test]
async fn a_cancelled_token_aborts_the_sweep_with_an_error_and_no_classification() {
    // Already-cancelled before the first rung: the probe must error out immediately
    // instead of running the ladder, so the command never persists a bogus status.
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let ladder = [0u32, 4000, 8000];
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut rungs = 0usize;
    let result = run_cliff_with(
        &model,
        "m",
        &tasks,
        &source(),
        &ladder,
        &DEFAULT_DEPTHS,
        &cancel,
        &mut |_, _, _| {
            rungs += 1;
        },
        &mut |_| {},
    )
    .await;
    assert!(result.is_err(), "a cancelled probe must return an error, not a report");
    assert_eq!(rungs, 0, "no rung should run once the token is cancelled");
}

#[tokio::test]
async fn the_needle_is_swept_across_all_default_depths() {
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks = [task()];
    let report = run_cliff(&model, "m", &tasks, &source(), &[4000u32], &DEFAULT_DEPTHS).await.unwrap();
    let rung = &report.points[0];
    assert_eq!(rung.per_depth.len(), DEFAULT_DEPTHS.len());
    let depths: Vec<f32> = rung.per_depth.iter().map(|d| d.depth).collect();
    assert_eq!(depths, DEFAULT_DEPTHS.to_vec());
}

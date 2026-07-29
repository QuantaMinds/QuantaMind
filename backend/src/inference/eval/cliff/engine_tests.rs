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
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let toks = (chars / 4) as u32;
        let text = if toks < self.threshold { self.good.clone() } else { "I cannot help with that.".to_string() };
        Ok((text, GenerateStats { prompt_eval_count: Some(toks), ..Default::default() }))
    }
}

/// A model behind a REAL context window, scripted from the live Ollama behaviour this
/// guards against: a prompt past `window` is not rejected — it is silently TRUNCATED to fit
/// (so the injected needle is dropped and the task can no longer be answered) and
/// `prompt_eval_count` saturates at `window` no matter how much padding was sent. Verified
/// live: requesting num_ctx 34816 on a 32768-window model returned n_ctx 32768, and 177 KB
/// vs 200 KB of padding both reported prompt_eval_count = 32768 with the needle gone.
struct TruncatingModel {
    window: u32,
    good: String,
}

impl ModelTurn for TruncatingModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let wanted = (chars / 4) as u32;
        // The window is a ceiling, not an error: the count saturates instead of growing.
        let toks = wanted.min(self.window);
        // Truncated ⇒ the needle was cut away ⇒ the model cannot produce the call.
        let text = if wanted > self.window { "I cannot help with that.".to_string() } else { self.good.clone() };
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

/// THE REGRESSION. A ladder that runs past the context window used to report a CLIFF at the
/// window — the model "failing" only because the harness truncated the needle out of its own
/// prompt, at a depth that was a saturated counter rather than a measurement. Both halves of
/// that rung are fabrications, so the engine must drop it and classify from the rungs it
/// really measured, never persisting a cliff the model never had.
#[tokio::test]
async fn a_rung_truncated_at_the_context_window_is_dropped_not_reported_as_a_cliff() {
    let window = 8_000u32;
    // The model itself never degrades — it answers correctly at every depth that FITS.
    let model = TruncatingModel { window, good: GOOD.into() };
    // The ladder walks past the window; the deep rungs can only saturate.
    let ladder = [0u32, 4_000, 12_000, 20_000];
    let mut emitted: Vec<u32> = Vec::new();
    let report = run_cliff_with(
        &model,
        "m",
        &[task()],
        &source(),
        &ladder,
        &DEFAULT_DEPTHS,
        window,
        CliffBudget::default(),
        &CancellationToken::new(),
        &mut |_, _, p: &CliffPoint| emitted.push(p.verified_tokens),
        &mut |_| {},
    )
    .await
    .unwrap();

    // No fabricated cliff: the model was healthy everywhere it could actually be measured.
    assert_eq!(report.status, CliffStatus::NoCliff { tested: report.points.last().unwrap().verified_tokens });
    // Every rung that survived is a real measurement — none sits at or past the window.
    for p in &report.points {
        assert!(
            measurable(p.verified_tokens, window, MAX_OUTPUT),
            "an unmeasurable rung reached the report: {} tokens vs a {window} window",
            p.verified_tokens,
        );
        assert!(p.verified_tokens < window, "a saturated depth was reported as verified");
    }
    // And it was never emitted to the live chart either.
    for t in &emitted {
        assert!(*t < window, "a saturated rung was streamed to the UI: {t}");
    }
}

/// The cap must not cost real coverage: a rung that FITS is measured and classified as
/// before. Without this, "drop the unmeasurable rung" could silently degrade into
/// "drop every padded rung" and the probe would measure nothing while still looking green.
#[tokio::test]
async fn rungs_that_fit_the_window_are_still_measured_and_can_still_find_a_real_cliff() {
    // Genuine collapse at 5000 tokens, well inside a 32k window — a real cliff, not an artifact.
    let model = CliffModel { threshold: 5_000, good: GOOD.into() };
    let report = run_cliff_with(
        &model,
        "m",
        &[task()],
        &source(),
        &[0u32, 4_000, 8_000],
        &DEFAULT_DEPTHS,
        32_768,
        CliffBudget::default(),
        &CancellationToken::new(),
        &mut |_, _, _| {},
        &mut |_| {},
    )
    .await
    .unwrap();
    assert!(matches!(report.status, CliffStatus::Collapsed { .. }), "a real cliff still reports: {:?}", report.status);
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
    assert_eq!(report.status, CliffStatus::Collapsed { depth: collapse_at, concentration: None });
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
    // The guard is "no FAKE Broken" — that a valid call is never mis-read as a bad abstention.
    // It deliberately does NOT assert `NoCliff`: this fixture is a ONE-task collection, whose
    // 3 pooled trials can't resolve the 0.2 collapse margin, so "no cliff" is a claim the
    // sample can't support and the engine now says `Inconclusive` instead. Asserting NoCliff
    // here would be re-asserting the very over-claim #158 is about.
    assert!(
        !matches!(report.status, CliffStatus::Broken { .. }),
        "a well-formed call must never read as a broken baseline: {:?}",
        report.status,
    );
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
    async fn run(&self, _spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
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
        NO_CTX_LIMIT,
        CliffBudget::default(),
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
        NO_CTX_LIMIT,
        CliffBudget::default(),
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
        NO_CTX_LIMIT,
        CliffBudget::default(),
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
        NO_CTX_LIMIT,
        CliffBudget::default(),
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
        NO_CTX_LIMIT,
        CliffBudget::default(),
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

// ── LIVE (ignored): the real engine, a real backend, a real model ────────────────────
//
// Rule 6: green unit tests only prove the path we scripted. This bug was invisible to
// them precisely because the scripted models had no context window to overflow — the
// harness truncating its own needle away is something only a real backend does.
//
// Run (Ollama):    cargo test --lib live_cliff_ollama -- --ignored --nocapture
// Run (llama.cpp): cargo test --lib live_cliff_llama  -- --ignored --nocapture
// Override with QM_LIVE_MODEL / QM_LIVE_CTX (llama.cpp: the server's launch `-c`).

/// The two facts a live run must establish, shared by both backends:
///   1. no rung is reported at/past the window (that rung was truncated, not measured);
///   2. no fabricated cliff — a cliff, if any, sits strictly inside the window.
fn assert_live_report_is_honest(report: &CliffReport, ctx_limit: u32, label: &str) {
    println!("\n=== LIVE cliff: {label} (window {ctx_limit}) ===");
    println!("status: {:?}  cliff_tokens: {:?}", report.status, report.cliff_tokens);
    for (i, p) in report.points.iter().enumerate() {
        println!(
            "  rung {i}: target={:>6} verified={:>6} composite={:?}",
            p.target_tokens, p.verified_tokens, p.composite
        );
    }
    for p in &report.points {
        assert!(
            measurable(p.verified_tokens, ctx_limit, MAX_OUTPUT),
            "{label}: an unmeasurable rung reached the report — verified={} vs window {ctx_limit}. \
             A prompt at the window was TRUNCATED by the backend, so its score and its depth are \
             both artifacts.",
            p.verified_tokens,
        );
    }
    if let CliffStatus::Collapsed { depth, .. } = report.status {
        assert!(
            depth < ctx_limit,
            "{label}: reported a cliff AT the context window ({depth} vs {ctx_limit}) — that is the \
             harness truncating its own needle, not a property of the model",
        );
    }
}

#[tokio::test]
#[ignore = "live: requires a running Ollama server and a pulled model"]
async fn live_cliff_ollama_reports_no_fabricated_cliff_at_the_window() {
    use crate::inference::backend::backend_kind::BackendKind;
    use crate::inference::eval::agentic::model_turn::BackendTurn;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5:3b".into());
    // The panel's fixed default: the window MINUS the headroom the backend adds.
    let window: u32 = std::env::var("QM_LIVE_CTX").ok().and_then(|s| s.parse().ok()).unwrap_or(8192);
    let max_tokens = window - 2048;

    let turn = BackendTurn {
        backend: BackendKind::Ollama,
        endpoint: "http://127.0.0.1:11434".into(),
        model: model.clone(),
        cancel: CancellationToken::new(),
        options: Some(GenerateOptions { num_ctx: Some(window), ..Default::default() }),
        keep_alive: None,
        is_thinking: false,
        max_tokens: 256,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };
    let ladder = build_ladder(max_tokens, 3);
    let report = run_cliff_with(
        &turn, &model, &[task()], &source(), &ladder, &DEFAULT_DEPTHS, window,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .expect("live Ollama probe");
    assert_live_report_is_honest(&report, window, &format!("ollama/{model}"));
}

#[tokio::test]
#[ignore = "live: requires llama-server on :8080 (--jinja) with the model loaded"]
async fn live_cliff_llama_reports_no_fabricated_cliff_at_the_window() {
    use crate::inference::backend::backend_kind::BackendKind;
    use crate::inference::eval::agentic::model_turn::BackendTurn;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder".into());
    // llama.cpp pins its window at launch, so this must match the server's `-c`.
    let window: u32 = std::env::var("QM_LIVE_CTX").ok().and_then(|s| s.parse().ok()).unwrap_or(16384);
    let max_tokens = window - 2048;

    let turn = BackendTurn {
        backend: BackendKind::LlamaCpp,
        endpoint: "http://127.0.0.1:8080".into(),
        model: model.clone(),
        cancel: CancellationToken::new(),
        options: Some(GenerateOptions { num_ctx: Some(window), ..Default::default() }),
        keep_alive: None,
        is_thinking: false,
        max_tokens: 256,
        cpu_offloaded: false,
        ctx_ceiling: crate::inference::eval::agentic::runner::NUM_CTX_CEILING,
        stop_cache: Default::default(),
    };
    let ladder = build_ladder(max_tokens, 3);
    let report = run_cliff_with(
        &turn, &model, &[task()], &source(), &ladder, &DEFAULT_DEPTHS, window,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .expect("live llama.cpp probe");
    assert_live_report_is_honest(&report, window, &format!("llama.cpp/{model}"));
}

// ── llama.cpp prompt-cache accounting ────────────────────────────────────────────────

/// A backend with a PROMPT CACHE, scripted from live llama-server behaviour: it reports
/// only the RECOMPUTED tokens in `prompt_eval_count` and the reused prefix in `cache_n`.
/// Measured live: the same 2457-token prompt reports prompt_n=2457 cold, then prompt_n=1
/// with cache_n=2456 warm. The cliff sweeps near-identical prompts, so it hits this on
/// nearly every turn after the first.
struct CachingModel {
    good: String,
}

impl ModelTurn for CachingModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let real = (chars / 4) as u32;
        // Warm cache: all but one token served from the reused prefix.
        Ok((
            self.good.clone(),
            GenerateStats { prompt_eval_count: Some(1), cache_n: Some(real.saturating_sub(1)), ..Default::default() },
        ))
    }
}

/// THE llama.cpp REGRESSION. Reading `prompt_eval_count` alone made a cache-served rung
/// look like a ~1-token prompt. That corrupted the depth (charted and persisted) AND the
/// learned byte→token rate — `bytes / 1` — which then sized the next rung far past the
/// window, at which point llama.cpp rejects the request outright and the whole probe dies
/// with "the prompt is larger than the context window". The depth must be the context the
/// model READ: `prompt_eval_count + cache_n`.
#[tokio::test]
async fn a_cache_served_prompt_is_measured_at_its_true_size_not_the_recomputed_part() {
    let model = CachingModel { good: GOOD.into() };
    let report = run_cliff_with(
        &model,
        "m",
        &[task()],
        &source(),
        &[0u32, 4_000],
        &DEFAULT_DEPTHS,
        32_768,
        CliffBudget::default(),
        &CancellationToken::new(),
        &mut |_, _, _| {},
        &mut |_| {},
    )
    .await
    .unwrap();
    let padded = &report.points[1];
    // Not ~1: the cached prefix counts, so the rung lands near its 4000-token target.
    assert!(
        padded.verified_tokens > 3_000,
        "a cache-served rung must report its TRUE prompt size, got {} tokens",
        padded.verified_tokens,
    );
}

/// The rebuild must never size a prompt past the window. Live, an overshoot was fatal:
/// llama.cpp answered a 9810-token prompt in an 8192 window with a hard error that aborted
/// the entire probe. A rung slightly under target beats no run at all.
#[test]
fn cap_bytes_never_sizes_a_prompt_past_the_window() {
    let rate = 5.0; // bytes per token
    let window = 8_192u32;
    // An overshooting rebuild (10k tokens' worth) is clamped to what the window holds.
    let capped = cap_bytes(50_000, rate, window, MAX_OUTPUT);
    let projected_tokens = (capped as f64 / rate).round() as u32;
    assert!(
        projected_tokens.saturating_add(MAX_OUTPUT) <= window,
        "{projected_tokens} tokens + reply must fit the {window} window",
    );
    // A size that already fits is left exactly alone — the cap must not cost real depth.
    assert_eq!(cap_bytes(10_000, rate, window, MAX_OUTPUT), 10_000);
    // Scripted turns have no real window to clamp against.
    assert_eq!(cap_bytes(usize::MAX, rate, NO_CTX_LIMIT, MAX_OUTPUT), usize::MAX);
}

// ── sample resolution: the composite must be able to support its own verdict ──────────

/// An agentic task (the shape every built-in v2 collection uses — `cliff_score` scores
/// these on well-formedness, `parsed / n`, which is what quantizes the composite).
fn agentic_tasks(n: usize) -> Vec<ToolTask> {
    (0..n)
        .map(|i| ToolTask {
            id: format!("t{i}"),
            category: "agent_loop".into(),
            prompt: format!("Do step {i}."),
            tools: vec![ToolSchema {
                name: "act".into(),
                description: "act".into(),
                parameters: json!({ "type": "object", "properties": { "x": { "type": "string" } } }),
            }],
            expected: Expected::Call(Call { name: "act".into(), args: json!({}) }),
            agentic: None,
        })
        .collect()
}

/// Fails exactly ONE task at exactly ONE needle position — the sporadic single flip a real
/// model produces (one fumbled quote, one bad sample). The position is recovered from WHERE
/// the injected task text sits in the padded prompt: `inject_at_depth` places the needle at
/// `len * depth`, so `find(task) / len` recovers the depth it was injected at.
struct OneFlipModel {
    flip_task: String,
    at_depth: f32,
    good: String,
}

impl ModelTurn for OneFlipModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let toks = ((spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len()) / 4) as u32;
        let where_injected = spec
            .prompt
            .find(&self.flip_task)
            .map(|i| i as f32 / spec.prompt.len().max(1) as f32);
        // Flip only this task, only at this needle depth. The unpadded baseline puts the task
        // at offset 0, so it never matches a mid/late depth → the baseline stays clean.
        let is_flip = where_injected.is_some_and(|p| (p - self.at_depth).abs() < 0.15);
        let text = if is_flip { "sorry, I cannot.".to_string() } else { self.good.clone() };
        Ok((text, GenerateStats { prompt_eval_count: Some(toks), ..Default::default() }))
    }
}

/// THE REGRESSION (#158). The default collection has 5 tasks, so a per-position score is
/// `parsed/5` — quantum 0.2, EXACTLY `COLLAPSE_MARGIN`. Taking the WORST of three positions
/// then meant a single task flipping at a single position produced `min(1.0, 0.8, 1.0) = 0.8`
/// against a 1.0 baseline: `0.8 <= 1.0 - 0.2` → `Collapsed`. One flake = a reported cliff, on
/// the shipped default. Pooling the positions (15 trials, quantum 0.067) is what makes a
/// single flip a 0.067 dent instead of a verdict.
#[tokio::test]
async fn a_single_task_flip_at_one_position_is_not_a_cliff() {
    let tasks = agentic_tasks(5);
    // ONE task fumbles at ONE position (the middle). Everything else is perfect.
    let model = OneFlipModel { flip_task: "Do step 3.".into(), at_depth: 0.5, good: GOOD.into() };
    let report = run_cliff_with(
        &model, "m", &tasks, &source(), &[0u32, 4_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    // Pooled: 14/15 = 0.933 vs a 1.0 baseline → a 0.067 dent, far under the 0.2 margin.
    // Worst-of-positions was min(1.0, 0.8, 1.0) = 0.8 → 0.8 <= 1.0 - 0.2 → a reported cliff.
    assert!(
        !matches!(report.status, CliffStatus::Collapsed { .. }),
        "one task fumbling at one position is noise, not a context cliff: {:?}",
        report.status,
    );
    let padded = &report.points[1];
    assert_eq!(padded.passed, Some(14), "14 of 15 trials passed");
    assert_eq!(padded.trials, Some(15));
}

/// The over-correction guard (passes before AND after — labelled as such, not dressed up as
/// a regression test). Pooling must not blunt a REAL positional collapse: a model that fails
/// every task at one position pools to 10/15 = 0.667, a 0.333 drop — always ≥ the 0.2 margin
/// for any n. The weakest-position signal survives; only the sporadic flip is filtered.
#[tokio::test]
async fn a_systematic_failure_at_one_position_is_still_a_cliff() {
    let tasks = agentic_tasks(5);
    // Fails EVERY task once padding exists (a real depth collapse).
    let model = CliffModel { threshold: 500, good: GOOD.into() };
    let report = run_cliff_with(
        &model, "m", &tasks, &source(), &[0u32, 4_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    assert!(
        matches!(report.status, CliffStatus::Collapsed { .. }),
        "a systematic collapse must still be caught: {:?}",
        report.status,
    );
}

/// A 1-task collection pools to 3 trials → quantum 0.333 > the 0.2 margin, so one flip and a
/// real collapse are the same measurement. Refuse. Reporting `NoCliff` would be an
/// affirmative claim the sample can't support; `Collapsed` would be a coin flip.
#[tokio::test]
async fn a_single_task_collection_cannot_resolve_the_margin() {
    let tasks = agentic_tasks(1);
    let model = CliffModel { threshold: 500, good: GOOD.into() }; // collapses when padded
    let report = run_cliff_with(
        &model, "m", &tasks, &source(), &[0u32, 4_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    assert!(
        matches!(report.status, CliffStatus::Inconclusive { .. }),
        "3 trials can't resolve a 0.2 margin — say so: {:?}",
        report.status,
    );
    assert_eq!(report.cliff_tokens, None, "an inconclusive probe reports no cliff depth");
}

/// The sample size must be MEASURED and carried, not inferred from `DEFAULT_DEPTHS` — the
/// rule "never report a number you didn't measure" applies to the denominator too.
#[tokio::test]
async fn a_rung_carries_its_measured_tally() {
    let tasks = agentic_tasks(5);
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let report = run_cliff_with(
        &model, "m", &tasks, &source(), &[0u32, 4_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    let padded = &report.points[1];
    assert_eq!(padded.trials, Some(15), "5 tasks × 3 positions, pooled");
    assert_eq!(padded.passed, Some(15), "a perfect model passes all of them");
    // The baseline is one position by construction, so its tally is smaller — carried honestly.
    assert_eq!(report.points[0].trials, Some(5));
}

// ── depth-scaled thinking budget ─────────────────────────────────────────────────────

/// A healthy scripted model that RECORDS each turn's `num_predict`, so the test can
/// assert what output budget the engine actually granted at every rung.
struct BudgetRecorder {
    good: String,
    seen: std::sync::Mutex<Vec<u32>>,
}

impl ModelTurn for &BudgetRecorder {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let np = spec.options.as_ref().and_then(|o| o.num_predict).unwrap_or(0);
        self.seen.lock().unwrap().push(np);
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        Ok((self.good.clone(), GenerateStats { prompt_eval_count: Some((chars / 4) as u32), ..Default::default() }))
    }
}

/// The thinking budget must scale with the RUNG's depth (banded through the canonical
/// tier table), while a non-thinking run keeps the flat answer floor at every depth —
/// byte-identical to the pre-preset probe.
#[tokio::test]
async fn thinking_budget_scales_per_rung_and_non_thinking_stays_flat() {
    use crate::inference::eval::agentic::difficulty::passk::ThinkPreset;
    let ladder = [0u32, 6_000, 12_000];

    let run = |budget: CliffBudget| async move {
        let model = BudgetRecorder { good: GOOD.into(), seen: std::sync::Mutex::new(Vec::new()) };
        run_cliff_with(
            &&model,
            "m",
            &[task()],
            &source(),
            &ladder,
            &DEFAULT_DEPTHS,
            NO_CTX_LIMIT,
            budget,
            &CancellationToken::new(),
            &mut |_, _, _| {},
            &mut |_| {},
        )
        .await
        .unwrap();
        let seen = model.seen.lock().unwrap().clone();
        seen
    };

    // Non-thinking: every turn runs at the flat answer floor.
    let flat = run(CliffBudget::default()).await;
    assert!(!flat.is_empty());
    assert!(flat.iter().all(|&np| np == CLIFF_ANSWER_TOKENS), "non-thinking must stay flat: {flat:?}");

    // Thinking (Standard): the granted budget grows band-by-band with the rung depth —
    // baseline (Easy band) < 6k (Medium band) < 12k (Hard band).
    let thinking = run(CliffBudget { is_thinking: true, preset: ThinkPreset::Standard, flat_cap: None }).await;
    let mut distinct: Vec<u32> = thinking.clone();
    distinct.dedup();
    let expected: Vec<u32> = ladder.iter().map(|&t| CliffBudget { is_thinking: true, preset: ThinkPreset::Standard, flat_cap: None }.max_output_for(t)).collect();
    assert_eq!(distinct, expected, "per-rung budgets must follow the depth bands: {thinking:?}");
    assert!(expected.windows(2).all(|w| w[0] < w[1]), "budget must increase with depth: {expected:?}");
}

/// The mode flag rides with the result: a thinking probe stamps its preset on the
/// report, a non-thinking probe stamps none — so a depth measured with a scratchpad
/// can never be conflated with one measured without.
#[tokio::test]
async fn report_carries_the_think_preset_only_when_thinking() {
    use crate::inference::eval::agentic::difficulty::passk::ThinkPreset;
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let plain = run_cliff_with(
        &model, "m", &[task()], &source(), &[0u32, 2_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget::default(),
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(plain.think_preset, None);

    let thinking = run_cliff_with(
        &model, "m", &[task()], &source(), &[0u32, 2_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT,
        CliffBudget { is_thinking: true, preset: ThinkPreset::Deep, flat_cap: None },
        &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {},
    )
    .await
    .unwrap();
    assert_eq!(thinking.think_preset, Some(ThinkPreset::Deep));
}

// ── per-task tally (by_task): the verdict layer sees WHICH tasks drove a rung ─────────

/// `by_task` must be the SAME measurement as the pooled rung tally, just grouped — on an
/// agentic-only rung the per-task counts sum exactly to `passed/trials`. Two aggregates
/// from one pass; if they can drift apart, the breakdown lies about the headline.
#[tokio::test]
async fn by_task_sums_to_the_pooled_rung_tally_and_names_the_failing_task() {
    // Two agentic tasks: one healthy at any depth, one that collapses past 3000 tokens.
    let model = CliffModel { threshold: 3_000, good: GOOD.into() };
    let healthy = agentic_task("stays-flat");
    let mut fragile = agentic_task("breaks-at-depth");
    // Longer prompt pushes ONLY this task's chars/4 over the scripted threshold at depth.
    fragile.prompt = "x".repeat(2_000);
    let report = run_cliff(&model, "m", &[healthy, fragile], &source(), &[0u32, 2_800], &DEFAULT_DEPTHS).await.unwrap();

    for p in &report.points {
        // Grouped == pooled, on every rung.
        let (sum_p, sum_t): (u32, u32) = p.by_task.iter().fold((0, 0), |(a, b), t| (a + t.passed, b + t.trials));
        assert_eq!(Some(sum_p), p.passed, "by_task passed must sum to the rung tally");
        assert_eq!(Some(sum_t), p.trials, "by_task trials must sum to the rung tally");
    }
    // The deep rung names the fragile task — and only it — as the failure source.
    let deep = report.points.last().unwrap();
    let failing: Vec<&str> = deep.by_task.iter().filter(|t| t.passed < t.trials).map(|t| t.task_id.as_str()).collect();
    assert_eq!(failing, vec!["breaks-at-depth"], "deep rung: {:?}", deep.by_task);
}

/// `by_task` is UNCAPPED: a collection larger than the trace cap still reports every task,
/// while `trace` keeps its `MAX_TRACE_TASKS` bound. Silent truncation of the breakdown would
/// re-create the exact blindness this field exists to remove.
#[tokio::test]
async fn by_task_reports_every_task_even_when_the_trace_caps() {
    let model = CliffModel { threshold: u32::MAX, good: GOOD.into() };
    let tasks: Vec<ToolTask> = (0..35)
        .map(|i| {
            let mut t = task();
            t.id = format!("t{i:02}");
            t
        })
        .collect();
    let report = run_cliff(&model, "m", &tasks, &source(), &[0u32], &DEFAULT_DEPTHS).await.unwrap();
    let p = &report.points[0];
    assert_eq!(p.trace.len(), 30, "trace stays capped at MAX_TRACE_TASKS");
    assert_eq!(p.by_task.len(), 35, "by_task must carry every task");
    assert!(p.by_task.iter().all(|t| t.trials == 1 && t.passed == 1));
}

// ── statistically honest collapse verdict: Newcombe gate + concentration ─────────────

/// A CliffPoint with a pooled tally + per-task breakdown, positions × tasks shaped like
/// a real agentic rung. `spec`: (task_id, passed, trials) per task.
fn point(tokens: u32, spec: &[(&str, u32, u32)]) -> CliffPoint {
    let by_task: Vec<TaskTally> =
        spec.iter().map(|(id, p, n)| TaskTally { task_id: (*id).into(), passed: *p, trials: *n, failed_cap_hits: 0, min_pass_headroom_milli: None }).collect();
    let (passed, trials) = by_task.iter().fold((0, 0), |(a, b), t| (a + t.passed, b + t.trials));
    CliffPoint {
        target_tokens: tokens,
        verified_tokens: tokens,
        composite: Some(passed as f64 / trials as f64),
        passed: Some(passed),
        trials: Some(trials),
        per_depth: vec![],
        trace: vec![],
        by_task,
        max_output: 256,
        cap_deaths: 0, // wire field only — verdict math derives from by_task
    }
}

/// THE reviewed run, as data: 5-trial baseline (one position), a 27pp drop at depth with
/// 3 of 4 failures in one task. The old verdict said "Collapsed at 8845"; the drop's
/// Newcombe interval includes zero (a 5-trial baseline can't anchor it), so the honest
/// verdict is Inconclusive — the exact over-claim the review caught.
#[test]
fn the_reviewed_runs_shape_is_inconclusive_not_a_collapse() {
    let base = point(704, &[("a", 1, 1), ("b", 1, 1), ("secret", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    let deep = point(8845, &[("a", 3, 3), ("b", 3, 3), ("secret", 0, 3), ("d", 3, 3), ("e", 2, 3)]);
    let (status, cliff) = classify(&[base, deep]);
    assert_eq!(status, CliffStatus::Inconclusive { trials: 15 }, "27pp off a 5-trial baseline is noise-indistinguishable");
    assert_eq!(cliff, None);
}

/// At the planned 18-task scale the SAME proportions resolve — and the verdict then
/// names the concentration: most failures from one task, with the leave-one-task-out
/// check showing the collapse does not survive that task's removal.
#[test]
fn a_resolved_concentrated_collapse_names_the_task_and_its_leverage() {
    // 18-task baseline all-pass; deep rung: one task 0/3, eleven other scattered failures
    // spread one-per-task (total 14 failures over 54 → 40/54 ≈ 74%, a 26pp resolved drop).
    let mut base_spec: Vec<(String, u32, u32)> = (0..18).map(|i| (format!("t{i:02}"), 1, 1)).collect();
    let mut deep_spec: Vec<(String, u32, u32)> = (0..18)
        .map(|i| {
            let id = format!("t{i:02}");
            if i == 7 { (id, 0, 3) } else if i < 12 { (id, 2, 3) } else { (id, 3, 3) }
        })
        .collect();
    let to_refs = |v: &Vec<(String, u32, u32)>| v.iter().map(|(s, p, n)| (s.clone(), *p, *n)).collect::<Vec<_>>();
    let (b, d) = (to_refs(&mut base_spec), to_refs(&mut deep_spec));
    let base = point(704, &b.iter().map(|(s, p, n)| (s.as_str(), *p, *n)).collect::<Vec<_>>());
    let deep = point(8845, &d.iter().map(|(s, p, n)| (s.as_str(), *p, *n)).collect::<Vec<_>>());
    let (status, _) = classify(&[base, deep]);
    match status {
        CliffStatus::Collapsed { depth: 8845, concentration } => {
            // 3 of 14 failures in one task is NOT ≥50%, and spread failures aren't
            // improbable under the null — so no concentration flag here.
            assert_eq!(concentration, None, "spread failures must not be flagged");
        }
        other => panic!("expected a resolved collapse, got {other:?}"),
    }
}

/// The concentration machinery itself, on the reviewed run's exact shape: names the
/// task, carries the hand-computed exact p (≈0.044 → 44 milli), and LOTO shows the
/// collapse rule does not survive that task's removal.
#[test]
fn concentration_for_names_the_task_and_loto_shows_single_task_leverage() {
    let base = point(704, &[("a", 1, 1), ("b", 1, 1), ("secret", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    let deep = point(8845, &[("a", 3, 3), ("b", 3, 3), ("secret", 0, 3), ("d", 3, 3), ("e", 2, 3)]);
    let c = concentration_for(&base, &deep).expect("3-of-4-in-one-task must flag");
    assert_eq!(c.task_id, "secret");
    assert_eq!((c.task_failures, c.total_failures), (3, 4));
    assert_eq!(c.p_value_milli, 44, "exact exchangeability p ≈ 0.044");
    assert!(c.holds_without, "excluding the task, the remaining tasks are flat — no collapse");
}

/// The path the interval gate CANNOT protect: a mixed/single-turn collection has no
/// summable tally (`passed`/`trials` None), so a margin drop still classifies Collapsed —
/// there the concentration label is the only honesty layer, and it must attach.
#[test]
fn no_tally_collapse_still_carries_the_concentration_label() {
    let mut base = point(704, &[("a", 1, 1), ("b", 1, 1), ("secret", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    let mut deep = point(8845, &[("a", 3, 3), ("b", 3, 3), ("secret", 0, 3), ("d", 3, 3), ("e", 2, 3)]);
    // Mixed collection: the pooled tally is not summable, only the graded composite exists.
    base.passed = None;
    base.trials = None;
    deep.passed = None;
    deep.trials = None;
    let (status, _) = classify(&[base, deep]);
    match status {
        CliffStatus::Collapsed { depth: 8845, concentration: Some(c) } => {
            assert_eq!(c.task_id, "secret");
            assert!(c.holds_without);
        }
        other => panic!("margin-only path must still label concentration: {other:?}"),
    }
}

/// Old persisted JSON (pre-concentration) parses unchanged — the field defaults to None,
/// and the legacy bare-number migration still lands on Collapsed.
#[test]
fn old_collapsed_json_round_trips_with_concentration_defaulting_to_none() {
    let old = r#"{"status":"Collapsed","depth":8845}"#;
    let parsed: CliffStatus = serde_json::from_str(old).unwrap();
    assert_eq!(parsed, CliffStatus::Collapsed { depth: 8845, concentration: None });
    // And a new record with concentration survives a round trip.
    let full = CliffStatus::Collapsed {
        depth: 8845,
        concentration: Some(crate::inference::eval::readiness::types::CliffConcentration {
            task_id: "secret_rotation".into(),
            task_failures: 3,
            total_failures: 4,
            p_value_milli: 44,
            holds_without: true,
        }),
    };
    let json = serde_json::to_string(&full).unwrap();
    assert_eq!(serde_json::from_str::<CliffStatus>(&json).unwrap(), full);
}

// ── Deliberation Headroom: cap-hit capture, BudgetLimited verdict, amber math ────────

/// Scripted model reproducing the wire shape of budget starvation: under `threshold`
/// tokens of context it answers correctly (finish "stop"); past it, it emits NOTHING
/// with finish "length" at exactly the cap — the signature measured live (all tokens
/// burned in the reasoning channel, guillotined before the first call).
struct CapStarvedModel {
    threshold: u32,
    good: String,
}

impl ModelTurn for CapStarvedModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let toks = (chars / 4) as u32;
        let cap = spec.options.as_ref().and_then(|o| o.num_predict).unwrap_or(256);
        if toks < self.threshold {
            Ok((self.good.clone(), GenerateStats {
                prompt_eval_count: Some(toks),
                eval_count: Some(cap / 2),
                finish_reason: Some("stop".into()),
                ..Default::default()
            }))
        } else {
            Ok((String::new(), GenerateStats {
                prompt_eval_count: Some(toks),
                eval_count: Some(cap),
                finish_reason: Some("length".into()),
                ..Default::default()
            }))
        }
    }
}

/// End-to-end: a run whose deep-rung failures ALL died at the cap must classify
/// BudgetLimited — never Collapsed — carrying the rung's cap. The exact
/// mis-attribution this feature exists to prevent, exercised through capture →
/// tally → classify, not constructed points.
#[tokio::test]
async fn all_cap_hit_failures_classify_budget_limited_not_collapsed() {
    let model = CapStarvedModel { threshold: 3_000, good: GOOD.into() };
    let tasks: Vec<ToolTask> = (0..5).map(|i| agentic_task(&format!("t{i}"))).collect();
    let report = run_cliff(&model, "m", &tasks, &source(), &[0u32, 4_000], &DEFAULT_DEPTHS).await.unwrap();
    match report.status {
        CliffStatus::BudgetLimited { depth, cap } => {
            assert!(depth > 3_000, "the budget-limited rung is the deep one: {depth}");
            assert_eq!(cap, 256, "carries the cap in force (the non-thinking answer floor)");
        }
        other => panic!("cap-starved failures must never read as a model verdict: {other:?}"),
    }
    assert_eq!(report.cliff_tokens, None, "no collapse depth is established");
    // The capture is visible per task: every deep failure is a counted cap-hit.
    let deep = report.points.last().unwrap();
    assert!(deep.by_task.iter().all(|t| t.trials - t.passed == t.failed_cap_hits));
    assert_eq!(deep.max_output, 256);
}

/// One failure NOT at the cap breaks the attribution: the rung falls through to the
/// normal (gated) collapse path. Absence of cap-hit data behaves the same — an old or
/// uninstrumented record can never claim BudgetLimited.
#[test]
fn mixed_or_uninstrumented_failures_never_claim_budget_limited() {
    let mk = |cap_hits: &[u32]| {
        let mut p = point(8_845, &[("a", 3, 3), ("b", 0, 3), ("c", 0, 3), ("d", 2, 3), ("e", 3, 3)]);
        for (t, &c) in p.by_task.iter_mut().zip(cap_hits) {
            t.failed_cap_hits = c;
        }
        p
    };
    let base = point(704, &[("a", 1, 1), ("b", 1, 1), ("c", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    // 7 failures, 6 cap-hits: one content failure ⇒ not budget-limited.
    let (status, _) = classify(&[base.clone(), mk(&[0, 3, 2, 1, 0])]);
    assert!(!matches!(status, CliffStatus::BudgetLimited { .. }), "mixed: {status:?}");
    // No cap data at all (old record) ⇒ not budget-limited either.
    let (status, _) = classify(&[base, mk(&[0, 0, 0, 0, 0])]);
    assert!(!matches!(status, CliffStatus::BudgetLimited { .. }), "uninstrumented: {status:?}");
}

/// A baseline failing purely at the cap is BudgetLimited, not Broken — "fails from
/// the start" must not be claimed when the harness never let it answer.
#[test]
fn cap_starved_baseline_is_budget_limited_not_broken() {
    let mut base = point(704, &[("a", 0, 1), ("b", 0, 1), ("c", 1, 1), ("d", 0, 1), ("e", 0, 1)]);
    for t in base.by_task.iter_mut() {
        t.failed_cap_hits = t.trials - t.passed;
    }
    let (status, _) = classify(&[base]);
    assert_eq!(status, CliffStatus::BudgetLimited { depth: 704, cap: 256 });
}

/// The amber math: headroom is ‰ of the cap left unused, folded as the MINIMUM over a
/// task's passing cells only; failures never contribute a headroom number.
#[test]
fn tally_headroom_is_min_over_passing_cells_only() {
    let pos = vec![
        PosTrace { task_id: "t".into(), prompt: String::new(), output: "x".into(), passed: true, decoded: Some(200), thinking: None, cap_hit: Some(false) },
        PosTrace { task_id: "t".into(), prompt: String::new(), output: "x".into(), passed: true, decoded: Some(240), thinking: None, cap_hit: Some(false) },
        PosTrace { task_id: "t".into(), prompt: String::new(), output: String::new(), passed: false, decoded: Some(256), thinking: None, cap_hit: Some(true) },
    ];
    let mut tally: Vec<TaskTally> = Vec::new();
    merge_pos_into_tally(&mut tally, &pos, 256);
    let t = &tally[0];
    assert_eq!((t.passed, t.trials, t.failed_cap_hits), (2, 3, 1));
    // 240/256 used ⇒ 16/256 left ⇒ 62‰ (floor) — under the 150‰ amber line.
    assert_eq!(t.min_pass_headroom_milli, Some(62));
    assert!(t.min_pass_headroom_milli.unwrap() < AMBER_HEADROOM_MILLI);
}

/// BudgetLimited round-trips serde and is distinct from every model verdict.
#[test]
fn budget_limited_serde_round_trip() {
    let s = CliffStatus::BudgetLimited { depth: 8845, cap: 256 };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("BudgetLimited"));
    assert_eq!(serde_json::from_str::<CliffStatus>(&json).unwrap(), s);
}

// ── baseline cap-headroom gate: a grazing baseline refuses the ladder at rung 0 ──────

/// Scripted model that always answers correctly but reports decoding `used` tokens
/// (finish "stop") — the wire shape of the live Leg-1 trap: a baseline that "passes
/// clean" while sitting at the edge of its output cap.
struct GrazingModel {
    used: u32,
    good: String,
}

impl ModelTurn for GrazingModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        Ok((self.good.clone(), GenerateStats {
            prompt_eval_count: Some((chars / 4) as u32),
            eval_count: Some(self.used),
            finish_reason: Some("stop".into()),
            ..Default::default()
        }))
    }
}

/// THE trap, as measured live (Qwen3.5-9B q4: a 5/5 baseline at 0‰ headroom turned
/// every deeper rung into cap-deaths): a baseline that passes AT the cap must refuse
/// the ladder at rung 0 — CapMarginal, with no padded rung paid for.
#[tokio::test]
async fn grazing_baseline_refuses_the_ladder_at_rung_zero() {
    let model = GrazingModel { used: 256, good: GOOD.into() };
    let tasks: Vec<ToolTask> = (0..5).map(|i| agentic_task(&format!("t{i}"))).collect();
    let report = run_cliff(&model, "m", &tasks, &source(), &[0u32, 4_000, 8_000], &DEFAULT_DEPTHS).await.unwrap();
    assert_eq!(report.status, CliffStatus::CapMarginal { cap: 256, used_milli: 1000 });
    assert_eq!(report.points.len(), 1, "no padded rung may be paid for after a grazing baseline");
    assert_eq!(report.cliff_tokens, None, "nothing above the baseline was measured");
}

/// The ≥0.9 boundary, pinned through a FLAT cap of 1000 (also exercising `--cap` in
/// the engine): "used 0.9 of the cap" rejects (the reviewer's rule verbatim), one
/// token under proceeds and measures the whole ladder.
#[tokio::test]
async fn cap_marginal_fires_at_exactly_nine_tenths_and_not_below() {
    let tasks: Vec<ToolTask> = (0..5).map(|i| agentic_task(&format!("t{i}"))).collect();
    let budget = CliffBudget { flat_cap: Some(1000), ..Default::default() };
    let probe = |used: u32| {
        let tasks = tasks.clone();
        async move {
            let model = GrazingModel { used, good: GOOD.into() };
            run_cliff_with(&model, "m", &tasks, &source(), &[0u32, 4_000, 8_000], &DEFAULT_DEPTHS, NO_CTX_LIMIT, budget, &CancellationToken::new(), &mut |_, _, _| {}, &mut |_| {})
                .await
                .unwrap()
        }
    };
    // 900/1000 ⇒ headroom exactly 100‰ ⇒ used_milli 900 — fires.
    let at = probe(900).await;
    assert_eq!(at.status, CliffStatus::CapMarginal { cap: 1000, used_milli: 900 });
    assert_eq!(at.points.len(), 1);
    // 899/1000 ⇒ 101‰ headroom — proceeds and the full ladder is measured.
    let under = probe(899).await;
    assert!(matches!(under.status, CliffStatus::NoCliff { .. }), "one token under the line must proceed: {:?}", under.status);
    assert_eq!(under.points.len(), 3, "every rung is paid for and measured");
}

/// Absence of measurement is never an attribution: a baseline whose cells reported no
/// decoded count (old/uninstrumented records — `point()` leaves headroom `None`) can
/// never fire the gate, however healthy or tight it might really have been.
#[test]
fn cap_marginal_never_fires_without_counts() {
    let base = point(704, &[("a", 1, 1), ("b", 1, 1), ("c", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    let (status, _) = classify(&[base]);
    assert!(!matches!(status, CliffStatus::CapMarginal { .. }), "uninstrumented: {status:?}");
}

/// CapMarginal round-trips serde (the persisted store and the GUI read this shape).
#[test]
fn cap_marginal_serde_round_trip() {
    let s = CliffStatus::CapMarginal { cap: 256, used_milli: 1000 };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("CapMarginal"));
    assert_eq!(serde_json::from_str::<CliffStatus>(&json).unwrap(), s);
}

// ── truncated-pass gate: a cell cut at the cap can never be scored a pass ────────────

/// Scripted model that emits a CORRECT, parseable call but reports `finish == "length"`
/// past `threshold` context — the wire shape of the contamination: the call is
/// well-formed, but the generation was guillotined, so whatever came next (a wrong
/// follow-up call, a self-correction) is censored. Under `threshold` it finishes clean.
struct TruncatedPassModel {
    threshold: u32,
    good: String,
}

impl ModelTurn for TruncatedPassModel {
    async fn run(&self, spec: &GenerateSpec, _progress: &Progress) -> AppResult<(String, GenerateStats)> {
        let chars = spec.system.as_deref().map_or(0, |s| s.len()) + spec.prompt.len();
        let toks = (chars / 4) as u32;
        let cap = spec.options.as_ref().and_then(|o| o.num_predict).unwrap_or(256);
        let truncated = toks >= self.threshold;
        Ok((self.good.clone(), GenerateStats {
            prompt_eval_count: Some(toks),
            eval_count: Some(if truncated { cap } else { cap / 4 }),
            finish_reason: Some(if truncated { "length" } else { "stop" }.into()),
            ..Default::default()
        }))
    }
}

/// Well-formed is not the same as complete: a rung whose cells all emit the CORRECT
/// call but die at the cap must classify BudgetLimited — the passes are artifacts of
/// censoring, never a measured NoCliff. Before the gate, this exact shape scored
/// 15/15 and the curve was part artifact (6 such cells sat in the live Leg-1 rung 2).
#[tokio::test]
async fn truncated_correct_output_never_scores_a_pass() {
    let model = TruncatedPassModel { threshold: 3_000, good: GOOD.into() };
    let tasks: Vec<ToolTask> = (0..5).map(|i| agentic_task(&format!("t{i}"))).collect();
    let report = run_cliff(&model, "m", &tasks, &source(), &[0u32, 4_000, 8_000], &DEFAULT_DEPTHS).await.unwrap();
    match report.status {
        CliffStatus::BudgetLimited { depth, cap } => {
            assert!(depth > 3_000, "the budget event is the deep rung: {depth}");
            assert_eq!(cap, 256);
        }
        other => panic!("truncated passes must read as a budget event, not a model verdict: {other:?}"),
    }
    // Every truncated cell is a died-at-cap, none is a pass — and the baseline
    // (clean finishes, quarter-cap usage) stays a full pass, so the gate cuts
    // exactly the censored cells and nothing else.
    let deep = report.points.last().unwrap();
    assert_eq!(deep.passed, Some(0), "a censored fragment is never a pass");
    assert_eq!(cap_deaths_of(deep), deep.trials.unwrap(), "all cells fold into died-at-cap");
    assert_eq!(report.points[0].passed, Some(5), "clean-finish baseline cells still pass");
    assert_eq!(report.cliff_tokens, None);
}

// ── three-bucket aggregate: cap-deaths enter neither numerator nor denominator ──────

/// Build a point and stamp its wire cap_deaths from the tallies (as the engine does).
fn point3(tokens: u32, spec: &[(&str, u32, u32, u32)]) -> CliffPoint {
    let mut p = point(tokens, &spec.iter().map(|(id, ps, tr, _)| (*id, *ps, *tr)).collect::<Vec<_>>());
    for (t, (_, _, _, cap)) in p.by_task.iter_mut().zip(spec) {
        t.failed_cap_hits = *cap;
    }
    p.cap_deaths = p.by_task.iter().map(|t| t.failed_cap_hits).sum();
    if p.cap_deaths > 0 {
        p.composite = None; // the engine blanks poolable cap-affected rungs
    }
    p
}

/// A rung that crosses the margin ONLY when its cap-deaths are folded in as failures
/// must not classify Collapsed — the collapse claim survives on content failures alone.
/// Content here: 10/11 ≈ 91% (no collapse); folded: 10/15 = 67% (would have collapsed).
#[test]
fn mixed_rung_that_only_collapses_when_folded_is_not_a_collapse() {
    let base = point(704, &[("a", 1, 1), ("b", 1, 1), ("c", 1, 1), ("d", 1, 1), ("e", 1, 1)]);
    let deep = point3(8845, &[("a", 3, 3, 0), ("b", 0, 3, 3), ("c", 2, 3, 1), ("d", 3, 3, 0), ("e", 2, 3, 0)]);
    let (status, _) = classify(&[base, deep.clone()]);
    assert!(
        matches!(status, CliffStatus::NoCliff { .. }),
        "content 10/11 holds; folding 4 cap-deaths in would fabricate a collapse: {status:?}"
    );
    // And the rung carries the honest triple, with no single rate.
    assert_eq!(deep.cap_deaths, 4);
    assert_eq!(deep.composite, None, "a cap-affected poolable rung has no one-number rate");
}

/// A REAL content collapse stays Collapsed even with cap-deaths alongside — excluding
/// the budget cells must never launder a genuine model collapse into health.
#[test]
fn content_collapse_survives_alongside_cap_deaths() {
    // 18-task scale so the content drop resolves: content 40/52 ≈ 77% vs 18/18 baseline
    // (>20pp, Newcombe-resolvable), plus 2 cap deaths that change nothing.
    let base_spec: Vec<(String, u32, u32)> = (0..18).map(|i| (format!("t{i:02}"), 1, 1)).collect();
    let mut deep_spec: Vec<(String, u32, u32, u32)> =
        (0..18).map(|i| (format!("t{i:02}"), 3, 3, 0)).collect();
    for i in 0..12 {
        deep_spec[i] = (format!("t{i:02}"), 2, 3, 0); // 12 content failures
    }
    deep_spec[16] = ("t16".into(), 2, 3, 1); // plus two cap deaths elsewhere
    deep_spec[17] = ("t17".into(), 2, 3, 1);
    let base = point(704, &base_spec.iter().map(|(s, p, n)| (s.as_str(), *p, *n)).collect::<Vec<_>>());
    let deep = point3(8845, &deep_spec.iter().map(|(s, p, n, c)| (s.as_str(), *p, *n, *c)).collect::<Vec<_>>());
    let (status, _) = classify(&[base, deep]);
    assert!(
        matches!(status, CliffStatus::Collapsed { .. }),
        "a content-resolved collapse must not be laundered by nearby cap deaths: {status:?}"
    );
}

/// A healthy-content baseline with one cap death anchors the run on its CONTENT counts
/// — it is neither Broken (content is fine) nor BudgetLimited (folded 4/5 ≥ floor).
#[test]
fn baseline_with_one_cap_death_and_healthy_content_still_anchors() {
    let base = point3(704, &[("a", 1, 1, 0), ("b", 1, 1, 0), ("c", 0, 1, 1), ("d", 1, 1, 0), ("e", 1, 1, 0)]);
    let deep = point(8845, &[("a", 3, 3), ("b", 3, 3), ("c", 3, 3), ("d", 3, 3), ("e", 3, 3)]);
    let (status, _) = classify(&[base, deep]);
    assert!(
        matches!(status, CliffStatus::NoCliff { .. }),
        "content 4/4 baseline anchors; one cap death is a budget note, not a verdict: {status:?}"
    );
}

//! Phase 7 acceptance suite — one integration test per end-to-end scenario (S1–S6
//! from the Phase-7 manual test plan), exercising the REAL pure functions the GUI
//! and the future CLI share. The truly manual legs (force-quit, live the server, real
//! VRAM detection) are out of scope here; this proves the deterministic logic each
//! scenario rests on. Scenario IDs map to the manual plan.

use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::eval::agentic::report::{AgenticReport, FailureKind, FailureTracker, RunOutcome, TopError};
use quantamind_lib::inference::eval::batch::{fold_report, AggAgentic, BatchColumn, BatchReport, CompletedUnit, TaskOutcome};
use quantamind_lib::inference::eval::readiness::inputs::{agentic_metrics, assess_report, verdict_for};
use quantamind_lib::inference::eval::readiness::profile::{builtins, ReadinessProfile};
use quantamind_lib::inference::eval::readiness::recommend;
use quantamind_lib::inference::eval::readiness::types::{AgentPath, ModelVerdict, Readiness};
use quantamind_lib::inference::eval::readiness::vram_fit::{estimate, try_profile, Dims};
use quantamind_lib::persistence::jobs::queue::{self, RunConfig};

// ── fixtures ─────────────────────────────────────────────────────────────────

/// An agentic aggregate with the metrics + failure counts a scenario needs.
fn agg(passes: u32, total: u32, steps: Option<f64>, effort: Option<f64>, loops: u32, halluc: u32) -> AggAgentic {
    AggAgentic {
        passes,
        total_runs: total,
        avg_steps: steps,
        avg_output_tokens_success: effort,
        schema_resilience: None,
        top_error: TopError::None,
        failures: FailureTracker {
            infinite_loop_hits: loops,
            hallucinated_completions: halluc,
            malformed_json_calls: 0,
            schema_unrecovered_calls: 0,
        },
    }
}

fn column(model: &str, agentic: Option<AggAgentic>, native: Option<AggAgentic>) -> BatchColumn {
    BatchColumn {
        model: model.into(),
        backend: BackendKind::LlamaCpp,
        toolcall: None,
        agentic,
        agentic_native_fc: native,
        error: None,
        is_thinking: false,
    }
}

/// A fully-controlled profile (explicit gates) so threshold tests are deterministic.
fn profile(min_pass_k: f64, forbid_loop: bool, require_vram: bool) -> ReadinessProfile {
    ReadinessProfile {
        id: "test".into(),
        name: "Test profile".into(),
        min_pass_k,
        max_avg_steps: None,
        max_ms_per_step: None,
        min_context_tokens: None,
        forbid_infinite_loop: forbid_loop,
        forbid_hallucinated_completion: false,
        require_full_vram: require_vram,
        require_native_fc: false,
    }
}

fn status_of(col: &BatchColumn, fits: Option<bool>, pressure: bool, p: &ReadinessProfile) -> Readiness {
    verdict_for(col, fits, pressure, None, p).status
}

// ── S1 — Readiness verdict + the page (7.1 + 7.7) ────────────────────────────

#[test]
fn s1_verdict_grades_ready_conditional_and_not_ready_with_real_reasons() {
    let p = profile(0.80, true, false);

    // Clean, high-pass model → Ready.
    let ready = column("clean", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), None);
    assert_eq!(status_of(&ready, Some(true), false, &p), Readiness::Ready);

    // Below the pass bar → NotReady, with the interpolated reason.
    let weak = column("weak", Some(agg(4, 10, Some(4.0), Some(120.0), 0, 0)), None);
    let v = verdict_for(&weak, Some(true), false, None, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(
        v.blocking.iter().any(|r| r.contains("0.4") && r.contains("0.8")),
        "expected an interpolated pass^k reason, got {:?}",
        v.blocking
    );

    // A looping model → NotReady (loop gate), even with a passing rate.
    let looper = column("looper", Some(agg(9, 10, Some(4.0), Some(120.0), 2, 0)), None);
    let lv = verdict_for(&looper, Some(true), false, None, &p);
    assert_eq!(lv.status, Readiness::NotReady);
    assert!(lv.blocking.iter().any(|r| r.to_lowercase().contains("loop")), "got {:?}", lv.blocking);
}

#[test]
fn s1_raising_min_pass_k_deterministically_flips_ready_to_not_ready() {
    let col = column("m", Some(agg(7, 10, Some(4.0), Some(120.0), 0, 0)), None); // pass^k = 0.70
    assert_eq!(status_of(&col, Some(true), false, &profile(0.60, true, false)), Readiness::Ready);
    assert_eq!(status_of(&col, Some(true), false, &profile(0.80, true, false)), Readiness::NotReady);
}

#[test]
fn s1_a_model_with_no_agentic_data_is_never_ready() {
    let bare = column("bare", None, None); // pass^k unmeasured
    assert_eq!(status_of(&bare, Some(true), false, &profile(0.80, true, false)), Readiness::NotReady);
}

// ── S2 — Hardware telemetry / VRAM fit (7.4) ─────────────────────────────────

const DIMS: Dims = Dims { layers: 32, head_count: 32, head_count_kv: 8, embedding_length: 4096, context_length: 8192 };

#[test]
fn s2_lowering_the_cap_flips_a_fitting_model_to_not_ready() {
    let weights = 8 * 1024u64.pow(3); // ~8 GB weights
    let roomy = estimate(weights, DIMS.layers, DIMS.head_count, DIMS.head_count_kv, DIMS.embedding_length, 8192, 24 * 1024u64.pow(3));
    assert!(roomy.fits, "8 GB model + cache must fit a 24 GB cap");

    let tight = estimate(weights, DIMS.layers, DIMS.head_count, DIMS.head_count_kv, DIMS.embedding_length, 8192, 8 * 1024u64.pow(3));
    assert!(!tight.fits, "the same model must NOT fit an 8 GB cap");

    // …and the verdict follows the fit under a require_full_vram profile.
    let col = column("m", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), None);
    let vram = profile(0.80, true, true);
    assert_eq!(status_of(&col, Some(true), false, &vram), Readiness::Ready); // fits → Ready
    assert_eq!(status_of(&col, Some(false), false, &vram), Readiness::NotReady); // won't fit → blocked
}

#[test]
fn s2_single_model_backend_is_unmeasured_and_blocks_under_require_full_vram() {
    // try_profile returns None when dims/weights/cap are absent (a non-the server backend).
    assert!(try_profile(None, None, None, Some(24 * 1024u64.pow(3))).is_none());

    // Unmeasured VRAM (None) under require_full_vram → NotReady (never a guessed pass).
    let col = column("remote-model", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), None);
    assert_eq!(status_of(&col, None, false, &profile(0.80, true, true)), Readiness::NotReady);
}

#[test]
fn s2_coding_agent_builtin_ships_require_full_vram_on() {
    let coding = builtins().into_iter().find(|p| p.id == "coding-agent").unwrap();
    assert!(coding.require_full_vram, "Coding agent must gate on VRAM fit (7.4)");
    let rag = builtins().into_iter().find(|p| p.id == "rag-assistant").unwrap();
    assert!(!rag.require_full_vram, "offload-tolerant profiles stay off");
}

// ── S3 — Native function-calling (7.2) ───────────────────────────────────────

#[test]
fn s3_native_pass_k_is_preferred_over_the_prompt_proxy() {
    // Prompt proxy passes (9/10), native fails (3/10). The verdict must use NATIVE.
    let col = column("m", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), Some(agg(3, 10, Some(6.0), Some(300.0), 0, 0)));
    let v = verdict_for(&col, Some(true), false, None, &profile(0.80, true, false));
    assert_eq!(v.status, Readiness::NotReady, "native 0.30 < 0.80 must block despite a passing prompt proxy");
    assert_eq!(v.path, AgentPath::NativeFc, "the path must be labelled native");

    // And the recommender's efficiency metrics come from the native aggregate.
    let (steps, effort) = agentic_metrics(&col);
    assert_eq!(steps, Some(6.0));
    assert_eq!(effort, Some(300.0));
}

#[test]
fn s3_prompt_only_model_is_labelled_prompt_based() {
    let col = column("m", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), None);
    assert_eq!(verdict_for(&col, Some(true), false, None, &profile(0.80, true, false)).path, AgentPath::PromptBased);
}

// ── S4 — Resumable queue + VRAM isolation (7.5) ──────────────────────────────

fn run_config() -> RunConfig {
    RunConfig {
        collection_id: "finance".into(),
        targets: vec![],
        tasks: vec![],
        k: Some(5),
        max_steps: Some(8),
        params: None,
        keep_alive: None,
        native: true,
    }
}

fn unit(task: &str, is_native: bool, passes: u32) -> CompletedUnit {
    let outcomes: Vec<RunOutcome> = (0..passes).map(|_| RunOutcome::success(2, 100)).collect();
    CompletedUnit {
        model: "qwen".into(),
        task_id: task.into(),
        category: "agentic".into(),
        outcome: TaskOutcome::Agentic { report: AgenticReport::from_outcomes(&outcomes) },
        is_native,
    }
}

#[test]
fn s4_queue_round_trips_a_header_with_prompt_and_native_units() {
    let dir = tempfile::tempdir().unwrap();
    let path = queue::run_path(dir.path(), "finance");
    queue::create(&path, &run_config()).unwrap();
    queue::append(&path, &unit("a1", false, 4)).unwrap(); // prompt
    queue::append(&path, &unit("a1", true, 2)).unwrap(); // native — first-class
    let (cfg, units) = queue::load(&path).unwrap().unwrap();
    assert_eq!(cfg.collection_id, "finance");
    assert!(cfg.native);
    assert_eq!(units.len(), 2);
    assert!(!units[0].is_native && units[1].is_native);
}

#[test]
fn s4_a_truncated_final_line_heals_rather_than_panicking() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = queue::run_path(dir.path(), "c");
    queue::create(&path, &run_config()).unwrap();
    queue::append(&path, &unit("a1", false, 3)).unwrap();
    // Simulate a hard crash mid-append: a half-written final JSON line.
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(f, "{{\"unit\":{{\"model\":\"qwen\",\"task_id\":\"a2\",\"cat").unwrap();
    let (_, units) = queue::load(&path).unwrap().unwrap();
    assert_eq!(units.len(), 1, "the broken tail is discarded; that unit just re-runs");
    assert_eq!(units[0].task_id, "a1");
}

#[test]
fn s4_fold_report_rebuilds_the_partial_matrix_for_bulk_rehydration() {
    let targets = vec![quantamind_lib::inference::eval::toolcall::matrix::ModelTarget {
        model: "qwen".into(),
        backend: BackendKind::LlamaCpp,
    }];
    let tasks = vec![task("a1")];
    let prior = vec![unit("a1", false, 2), unit("a1", true, 1)]; // prompt 2/2 + native 1/1
    let report = fold_report("finance", &targets, &tasks, &prior);
    let col = &report.columns[0];
    assert_eq!(col.agentic.as_ref().unwrap().passes, 2, "prompt units fold into agentic");
    assert_eq!(col.agentic_native_fc.as_ref().unwrap().passes, 1, "native units fold into agentic_native_fc");
}

fn task(id: &str) -> quantamind_lib::inference::eval::toolcall::tasks::ToolTask {
    use quantamind_lib::inference::eval::toolcall::tasks::{Expected, ToolTask};
    ToolTask {
        id: id.into(),
        category: "agentic".into(),
        prompt: "p".into(),
        tools: vec![],
        expected: Expected::NoCall,
        agentic: None,
    }
}

// ── S5 — Agentic-aware recommender (7.3) ─────────────────────────────────────

fn verdict(model: &str, status: Readiness, effort: Option<f64>, steps: Option<f64>) -> ModelVerdict {
    use quantamind_lib::inference::eval::readiness::types::ReadinessVerdict;
    ModelVerdict {
        model: model.into(),
        backend: BackendKind::LlamaCpp,
        verdict: ReadinessVerdict { status, blocking: vec![], conditions: vec![], path: AgentPath::PromptBased },
        memory: None,
        avg_steps: steps,
        effort,
        pass_k: None,
        quantization: None,
        cliff_tokens: None,
    }
}

#[test]
fn s5_recommender_ranks_ready_first_then_lower_effort_then_fewer_steps() {
    let mut v = vec![
        verdict("not_ready", Readiness::NotReady, Some(1.0), Some(1.0)),
        verdict("ready_costly", Readiness::Ready, Some(500.0), Some(2.0)),
        verdict("ready_cheap", Readiness::Ready, Some(200.0), Some(9.0)), // higher steps, lower effort → wins tier
        verdict("conditional", Readiness::Conditional, Some(1.0), Some(1.0)),
    ];
    recommend::rank(&mut v);
    let order: Vec<&str> = v.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(order, vec!["ready_cheap", "ready_costly", "conditional", "not_ready"]);
    assert_eq!(recommend::recommendation(&v).unwrap().model, "ready_cheap");
}

#[test]
fn s5_all_not_ready_recommends_the_closest_and_never_panics_on_unmeasured() {
    let mut v = vec![
        verdict("no_metrics", Readiness::NotReady, None, None), // unmeasured → sinks
        verdict("measured", Readiness::NotReady, Some(300.0), Some(3.0)),
    ];
    recommend::rank(&mut v);
    // Still NotReady, but the "closest" (measured) is surfaced first — no fabricated Ready.
    assert_eq!(recommend::recommendation(&v).unwrap().model, "measured");
    assert_eq!(recommend::recommendation(&v).unwrap().verdict.status, Readiness::NotReady);
}

// ── S6 — Integration / honesty sweep (cross-cutting) ─────────────────────────

#[test]
fn s6_required_but_unmeasured_metric_blocks_it_is_not_a_soft_pass() {
    // A profile requiring context headroom, which the engine never measured → NotReady.
    let mut p = profile(0.80, true, false);
    p.min_context_tokens = Some(8000);
    let col = column("m", Some(agg(9, 10, Some(4.0), Some(120.0), 0, 0)), None);
    let v = verdict_for(&col, Some(true), false, None, &p);
    assert_eq!(v.status, Readiness::NotReady);
    assert!(
        v.blocking.iter().any(|r| r.to_lowercase().contains("context") || r.to_lowercase().contains("measur")),
        "expected an unmeasured-context block, got {:?}",
        v.blocking
    );
}

#[test]
fn s6_end_to_end_report_walks_verdict_then_recommend_into_a_ranked_leaderboard() {
    // A realistic multi-model report → assess_report → rank: the integration the
    // Agent Report page renders. Proves the pieces compose, best-first.
    let report = BatchReport {
        collection_id: "c".into(),
        num_ctx: Some(8192),
        columns: vec![
            column("loops", Some(agg(9, 10, Some(4.0), Some(120.0), 3, 0)), None), // NotReady (loops)
            column("cheap-ready", Some(agg(9, 10, Some(3.0), Some(150.0), 0, 0)), None), // Ready, low effort
            column("costly-ready", Some(agg(9, 10, Some(3.0), Some(400.0), 0, 0)), None), // Ready, higher effort
            column("bare", None, None), // NotReady (no data)
        ],
    };
    let mut verdicts = assess_report(&report, &profile(0.80, true, false));
    recommend::rank(&mut verdicts);
    let order: Vec<&str> = verdicts.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(order[0], "cheap-ready", "the lowest-effort Ready model tops the leaderboard");
    assert_eq!(order[1], "costly-ready");
    assert_eq!(verdicts[0].verdict.status, Readiness::Ready);
    // Both NotReady models sink to the bottom.
    assert_eq!(verdicts[2].verdict.status, Readiness::NotReady);
    assert_eq!(verdicts[3].verdict.status, Readiness::NotReady);
}

// A loop-failure must surface a real failure kind in the report (not a synthesized
// pass) — backs the "never fabricate" rule end-to-end.
#[test]
fn s6_a_loop_failure_is_recorded_not_smoothed_over() {
    let r = AgenticReport::from_outcomes(&[
        RunOutcome::success(2, 100),
        RunOutcome::failure(8, 50, FailureKind::InfiniteLoop),
    ]);
    assert_eq!(r.passes, 1);
    assert_eq!(r.failures.infinite_loop_hits, 1);
}

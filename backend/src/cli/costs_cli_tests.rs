use super::*;
use crate::inference::eval::agentic::scoring::report::AgenticReport;
use crate::inference::eval::agentic::step::{StepKind, TrajectoryStep};
use crate::persistence::jobs::transcripts::{append_outcome, append_step, begin_task, transcript_path};
use tempfile::tempdir;

fn step(run: u32, idx: u32) -> TrajectoryStep {
    TrajectoryStep {
        run_index: run,
        step_index: idx,
        raw_output: "{}".into(),
        injection: None,
        kind: StepKind::ToolCall,
        env: Default::default(),
        cache_n: Some(100),
        prefill_tokens: Some(20),
        prefill_ms: Some(500),
        eval_ms: Some(900),
        load_ms: None,
        total_ms: None,
        output_tokens: Some(40),
        resident_bytes: Some(4_000_000_000),
        reasoning_tokens: None,
        thinking_split_measured: false,
        context_used: None,
        context_window: None,
        initial_prompt: None,
    }
}

/// Transcript stems split unambiguously back into (model, task, pass).
#[test]
fn stems_parse_model_task_and_pass() {
    assert_eq!(parse_stem("qwen3-5-9b-4ddc45bd--es-rs-search-fact-ddc289e7"), Some(("qwen3-5-9b-4ddc45bd".into(), "es-rs-search-fact-ddc289e7".into(), false)));
    assert_eq!(parse_stem("m-1--t-2--native"), Some(("m-1".into(), "t-2".into(), true)));
    assert_eq!(parse_stem("no-separator"), None);
}

/// End-to-end offline read: what the app's sink persisted comes back as cost rows —
/// per pass, with measured sums intact and honest gaps intact.
#[test]
fn persisted_run_reads_back_as_cost_rows() {
    let dir = tempdir().unwrap();
    let tdir = dir.path().join("agentic_transcripts");
    // The app writes via the same transcript_path this reader mirrors.
    let p = transcript_path(&tdir, "my-coll", "qwen3.5:9b", "task_a", false);
    begin_task(&p).unwrap();
    append_step(&p, &step(0, 0)).unwrap();
    append_step(&p, &step(0, 1)).unwrap();
    append_outcome(&p, &crate::inference::eval::batch::TaskOutcome::Agentic { report: AgenticReport::from_outcomes(&[]).with_wall_ms(4200) }).unwrap();
    let n = transcript_path(&tdir, "my-coll", "qwen3.5:9b", "task_a", true);
    begin_task(&n).unwrap();
    append_step(&n, &step(0, 0)).unwrap();

    let runs = load_collection_costs(dir.path(), "my-coll").unwrap();
    assert_eq!(runs.len(), 1, "one model");
    let tasks = &runs[0].tasks;
    assert_eq!(tasks.len(), 2, "prompt + native rows, never blended");
    let prompt = tasks.iter().find(|t| t.pass == "prompt").unwrap();
    assert_eq!(prompt.steps, 2);
    assert_eq!(prompt.prefill_ms_total, Some(1000));
    assert_eq!(prompt.cache_hit_tokens_total, Some(200));
    assert_eq!(prompt.wall_ms, Some(4200));
    assert_eq!(prompt.peak_context_tokens, Some(160)); // 100 + 20 + 40
    assert!(tasks.iter().any(|t| t.pass == "native"));
    // No batch report on disk → memory facts stay honestly unmeasured.
    assert_eq!(runs[0].memory.model_bytes, None);
}

/// A collection that never ran errors with a pointer, never an empty fabrication.
#[test]
fn missing_collection_is_a_loud_error() {
    let dir = tempdir().unwrap();
    let err = load_collection_costs(dir.path(), "never-ran").unwrap_err();
    assert!(err.to_string().contains("never-ran"));
}

use super::*;
use crate::inference::eval::agentic::step::StepKind;
use crate::inference::eval::batch::TaskOutcome;
use serde_json::Value;
use tempfile::tempdir;

fn step(run_index: u32, step_index: u32, raw: &str) -> TrajectoryStep {
    TrajectoryStep {
        run_index,
        step_index,
        raw_output: raw.into(),
        injection: Some(r#"{"status":"delivered"}"#.into()),
        kind: StepKind::ToolCall,
        env: Default::default(),
        cache_n: None,
        prefill_tokens: None,
        prefill_ms: None,
        reasoning_tokens: None,
        context_used: None,
        context_window: None,
    }
}

fn lines(path: &std::path::Path) -> Vec<Value> {
    read(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn a_transcript_round_trips_steps_then_outcome_in_order() {
    let dir = tempdir().unwrap();
    let path = transcript_path(dir.path(), "hard-support-ecommerce", "qwen3.5:9b", "hd_se_returns_instance0", false);
    begin_task(&path).unwrap();
    append_step(&path, &step(0, 0, r#"{"name":"get_order","args":{"id":"O-1"}}"#)).unwrap();
    append_step(&path, &step(0, 1, r#"{"name":"full_refund","args":{"order_id":"O-1"}}"#)).unwrap();
    append_outcome(&path, &TaskOutcome::Error { message: "boom".into() }).unwrap();

    let recs = lines(&path);
    assert_eq!(recs.len(), 3, "two steps + one outcome");
    // Shape AND values: steps carry the raw model output + injection, in order.
    assert_eq!(recs[0]["step"]["step_index"], 0);
    assert_eq!(recs[1]["step"]["step_index"], 1);
    assert!(recs[0]["step"]["raw_output"].as_str().unwrap().contains("get_order"));
    assert!(recs[0]["step"]["injection"].as_str().unwrap().contains("delivered"));
    // The outcome is the LAST line, self-describing (internally kind-tagged).
    assert_eq!(recs[2]["outcome"]["kind"], "error");
    assert!(recs[2]["outcome"]["message"].as_str().unwrap().contains("boom"));
}

#[test]
fn begin_task_truncates_the_previous_batch_latest_only_retention() {
    let dir = tempdir().unwrap();
    let path = transcript_path(dir.path(), "c", "m", "t", false);
    begin_task(&path).unwrap();
    append_step(&path, &step(0, 0, "old run")).unwrap();
    // A new batch on the same (collection, model, task) starts fresh.
    begin_task(&path).unwrap();
    append_step(&path, &step(0, 0, "new run")).unwrap();
    let recs = lines(&path);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["step"]["raw_output"], "new run");
}

#[test]
fn model_names_with_separators_map_to_safe_distinct_paths() {
    let dir = tempdir().unwrap();
    let a = transcript_path(dir.path(), "c", "qwen3:8b", "t", false);
    let b = transcript_path(dir.path(), "c", "qwen3/8b", "t", false);
    // Both are safe (no raw `:` or `/` path segments beyond the intended dirs)…
    for p in [&a, &b] {
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(!name.contains(':') && !name.contains('/'), "unsafe filename: {name}");
        begin_task(p).unwrap(); // and actually creatable
    }
    // …and distinct (the collision-proof hash suffix), so results never mix.
    assert_ne!(a, b);
}

#[test]
fn native_and_prompt_passes_get_separate_transcripts() {
    let dir = tempdir().unwrap();
    let prompt = transcript_path(dir.path(), "c", "m", "t", false);
    let native = transcript_path(dir.path(), "c", "m", "t", true);
    assert_ne!(prompt, native);
    assert!(native.file_name().unwrap().to_str().unwrap().ends_with("--native.jsonl"));
}

#[test]
fn oversize_transcript_read_is_capped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("big.jsonl");
    std::fs::write(&path, vec![b' '; (MAX_READ_BYTES + 1) as usize]).unwrap();
    assert!(matches!(read(&path), Err(AppError::Validation(_))));
}

#[test]
fn an_unwritable_dir_errors_instead_of_panicking() {
    // The sink treats this Err as warn-and-continue; here we just prove the
    // store surfaces it as an Err (no panic, no partial state). The unwritable
    // path is a regular FILE used as a directory component — invalid on every
    // platform (a unix-only path like /dev/null/... is creatable on Windows).
    let dir = tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "a file, not a dir").unwrap();
    let path = blocker.join("sub").join("t.jsonl");
    assert!(begin_task(&path).is_err());
    assert!(append_step(&path, &step(0, 0, "x")).is_err());
}

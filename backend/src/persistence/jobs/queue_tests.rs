use super::*;
use crate::inference::eval::batch::{CompletedUnit, TaskOutcome};
use std::io::Write;
use tempfile::tempdir;

fn config() -> RunConfig {
    RunConfig {
        collection_id: "finance".into(),
        targets: vec![],
        tasks: vec![],
        k: Some(5),
        max_steps: Some(8),
        params: None,
        keep_alive: None,
        native: true,
        prompt: true,
        tier: None,
        decoy_tools: None,
        think_preset: Default::default(),
    }
}

fn unit(task: &str, native: bool) -> CompletedUnit {
    CompletedUnit {
        model: "qwen".into(),
        task_id: task.into(),
        category: "agentic".into(),
        outcome: TaskOutcome::Error { message: "boom".into() },
        is_native: native,
    }
}

#[test]
fn header_and_prompt_plus_native_units_round_trip() {
    let dir = tempdir().unwrap();
    let path = run_path(dir.path(), "finance");
    create(&path, &config()).unwrap();
    append(&path, &unit("t1", false)).unwrap(); // prompt
    append(&path, &unit("t1", true)).unwrap(); // native
    let (cfg, units) = load(&path).unwrap().unwrap();
    assert_eq!(cfg.collection_id, "finance");
    assert_eq!(cfg.k, Some(5));
    assert!(cfg.native);
    assert_eq!(units.len(), 2);
    assert!(!units[0].is_native);
    assert!(units[1].is_native); // native FC is a first-class queue citizen
}

#[test]
fn append_is_additive_not_a_full_rewrite() {
    let dir = tempdir().unwrap();
    let path = run_path(dir.path(), "c");
    create(&path, &config()).unwrap();
    append(&path, &unit("t1", false)).unwrap();
    append(&path, &unit("t2", false)).unwrap();
    let (_, units) = load(&path).unwrap().unwrap();
    assert_eq!(units.iter().map(|u| u.task_id.as_str()).collect::<Vec<_>>(), vec!["t1", "t2"]);
}

#[test]
fn truncated_final_line_is_healed_not_panicked() {
    let dir = tempdir().unwrap();
    let path = run_path(dir.path(), "c");
    create(&path, &config()).unwrap();
    append(&path, &unit("t1", false)).unwrap();
    // Simulate a hard crash mid-append: a half-written final JSON line, no newline.
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(f, "{{\"unit\":{{\"model\":\"qwen\",\"task_id\":\"t2\",\"cat").unwrap();
    let (cfg, units) = load(&path).unwrap().unwrap();
    assert_eq!(cfg.collection_id, "finance");
    assert_eq!(units.len(), 1); // the broken tail (t2) is discarded — that unit just re-runs
    assert_eq!(units[0].task_id, "t1");
}

#[test]
fn load_missing_is_none() {
    let dir = tempdir().unwrap();
    assert!(load(&run_path(dir.path(), "never")).unwrap().is_none());
}

#[test]
fn list_paths_finds_each_run_log() {
    let dir = tempdir().unwrap();
    create(&run_path(dir.path(), "a"), &config()).unwrap();
    create(&run_path(dir.path(), "b"), &config()).unwrap();
    assert_eq!(list_paths(dir.path()).unwrap().len(), 2);
}

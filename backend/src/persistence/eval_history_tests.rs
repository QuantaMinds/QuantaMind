use super::*;
use crate::inference::backend::backend_kind::BackendKind;
use tempfile::tempdir;

fn summary(ts: &str, composite: f64) -> RunSummary {
    RunSummary {
        ts: ts.into(),
        model: "llama3.2:1b".into(),
        backend: BackendKind::LlamaCpp,
        parse_rate: Some(1.0),
        tool_selection_acc: Some(1.0),
        arg_acc: Some(0.5),
        abstain_acc: None,
        composite: Some(composite),
        n: 3,
        pass_k: None,
        agentic_avg_steps: None,
        effort: None,
        is_thinking: false,
    }
}

#[test]
fn old_summary_without_agentic_fields_still_loads() {
    // Back-compat: history written before Phase 6 omits pass_k/agentic_avg_steps/
    // effort; #[serde(default)] must fill them with None.
    let json = r#"[{"ts":"t","model":"m","backend":"llama_cpp","parse_rate":1.0,"tool_selection_acc":1.0,"arg_acc":1.0,"abstain_acc":null,"composite":0.9,"n":2}]"#;
    let parsed: Vec<RunSummary> = serde_json::from_str(json).unwrap();
    assert_eq!(parsed[0].pass_k, None);
    assert_eq!(parsed[0].effort, None);
}

#[test]
fn load_missing_is_empty() {
    let dir = tempdir().unwrap();
    let h = load(&dir.path().join("history"), "mine").unwrap();
    assert!(h.entries.is_empty());
    assert_eq!(h.unreadable, 0);
}

#[test]
fn appends_without_overwriting_past_runs() {
    let dir = tempdir().unwrap();
    append(dir.path(), "mine", &[summary("t1", 0.8)]).unwrap();
    append(dir.path(), "mine", &[summary("t2", 0.9)]).unwrap();
    let all = load(dir.path(), "mine").unwrap().entries;
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].ts, "t1");
    assert_eq!(all[1].ts, "t2");
}

#[test]
fn truncates_at_max_cap() {
    let dir = tempdir().unwrap();
    let batch: Vec<RunSummary> = (0..MAX_ENTRIES + 5).map(|i| summary(&format!("t{i}"), 0.5)).collect();
    append(dir.path(), "mine", &batch).unwrap();
    let all = load(dir.path(), "mine").unwrap().entries;
    assert_eq!(all.len(), MAX_ENTRIES);
    // Oldest dropped: the first kept entry is t5 (indices 0..4 evicted).
    assert_eq!(all[0].ts, "t5");
    assert_eq!(all[MAX_ENTRIES - 1].ts, format!("t{}", MAX_ENTRIES + 4));
}

#[test]
fn bad_collection_id_rejected() {
    let dir = tempdir().unwrap();
    for id in ["../escape", "a/b", "", "..", ".hidden"] {
        assert!(append(dir.path(), id, &[summary("t", 0.5)]).is_err(), "should reject {id:?}");
        assert!(load(dir.path(), id).is_err());
    }
}

/// The migration case that broke the Results History panel: a run recorded when a
/// now-removed backend existed. One such row must cost only itself — the panel
/// still renders every readable run — and must be COUNTED so the UI can say so
/// instead of quietly showing a short list.
#[test]
fn a_record_naming_an_unsupported_backend_is_skipped_not_fatal() {
    let dir = tempdir().unwrap();
    let json = r#"[
        {"ts":"t1","model":"m","backend":"retired_engine","parse_rate":1.0,"tool_selection_acc":1.0,"arg_acc":1.0,"abstain_acc":null,"composite":0.9,"n":2},
        {"ts":"t2","model":"m","backend":"llama_cpp","parse_rate":1.0,"tool_selection_acc":1.0,"arg_acc":1.0,"abstain_acc":null,"composite":0.7,"n":2}
    ]"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("mine.json"), json).unwrap();

    let h = load(dir.path(), "mine").unwrap();
    assert_eq!(h.entries.len(), 1, "the readable run still loads");
    assert_eq!(h.entries[0].ts, "t2");
    assert_eq!(h.unreadable, 1, "the legacy row is counted, not silently dropped");
}

/// Appending must not quietly delete the rows this build couldn't interpret —
/// the file is the user's record, and a future build (or a bug fix) may read them
/// again. The rewrite carries them through untouched.
#[test]
fn appending_preserves_records_this_build_cannot_read() {
    let dir = tempdir().unwrap();
    let json = r#"[{"ts":"old","model":"m","backend":"retired_engine","parse_rate":1.0,"tool_selection_acc":1.0,"arg_acc":1.0,"abstain_acc":null,"composite":0.9,"n":2}]"#;
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("mine.json"), json).unwrap();

    append(dir.path(), "mine", &[summary("new", 0.6)]).unwrap();

    let raw = std::fs::read_to_string(dir.path().join("mine.json")).unwrap();
    assert!(raw.contains("retired_engine"), "legacy row survived the rewrite: {raw}");
    let h = load(dir.path(), "mine").unwrap();
    assert_eq!(h.entries.len(), 1);
    assert_eq!(h.entries[0].ts, "new");
    assert_eq!(h.unreadable, 1);
}

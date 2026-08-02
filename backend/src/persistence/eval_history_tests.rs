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
    assert!(load(&dir.path().join("history"), "mine").unwrap().is_empty());
}

#[test]
fn appends_without_overwriting_past_runs() {
    let dir = tempdir().unwrap();
    append(dir.path(), "mine", &[summary("t1", 0.8)]).unwrap();
    append(dir.path(), "mine", &[summary("t2", 0.9)]).unwrap();
    let all = load(dir.path(), "mine").unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].ts, "t1");
    assert_eq!(all[1].ts, "t2");
}

#[test]
fn truncates_at_max_cap() {
    let dir = tempdir().unwrap();
    let batch: Vec<RunSummary> = (0..MAX_ENTRIES + 5).map(|i| summary(&format!("t{i}"), 0.5)).collect();
    append(dir.path(), "mine", &batch).unwrap();
    let all = load(dir.path(), "mine").unwrap();
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

use super::*;
use crate::inference::eval::toolcall::tasks::{Call, Expected, ToolSchema, ToolTask};
use serde_json::json;
use tempfile::tempdir;

fn sample() -> Vec<ToolTask> {
    vec![ToolTask {
        id: "w".into(),
        category: "single".into(),
        prompt: "Weather in Paris?".into(),
        tools: vec![ToolSchema {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }),
        }],
        expected: Expected::Call(Call { name: "get_weather".into(), args: json!({ "city": "Paris" }) }),
        agentic: None,
    }]
}

#[test]
fn list_missing_dir_is_empty() {
    let dir = tempdir().unwrap();
    assert!(list(&dir.path().join("evals")).unwrap().is_empty());
}

#[test]
fn save_then_list_load_round_trips() {
    let dir = tempdir().unwrap();
    save(dir.path(), "my_suite", &sample()).unwrap();
    assert_eq!(list(dir.path()).unwrap(), vec!["my_suite".to_string()]);
    assert_eq!(load(dir.path(), "my_suite").unwrap(), sample());
}

#[test]
fn delete_removes_collection() {
    let dir = tempdir().unwrap();
    save(dir.path(), "my_suite", &sample()).unwrap();
    delete(dir.path(), "my_suite").unwrap();
    assert!(list(dir.path()).unwrap().is_empty());
    assert!(matches!(delete(dir.path(), "my_suite"), Err(AppError::NotFound(_))));
}

#[test]
fn load_malformed_json_errors() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
    assert!(matches!(load(dir.path(), "broken"), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn load_invalid_task_rejected() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    // valid JSON, but category 'abstain' with a Call expected → invalid task
    let bad = json!([{ "id": "x", "category": "abstain", "prompt": "p",
        "tools": [{ "name": "t", "description": "d", "parameters": { "type": "object", "properties": {} } }],
        "expected": { "type": "call", "name": "t", "args": {} } }]);
    std::fs::write(dir.path().join("bad.json"), bad.to_string()).unwrap();
    assert!(matches!(load(dir.path(), "bad"), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn oversize_file_is_capped() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    let big = vec![b' '; (MAX_BYTES + 1) as usize];
    std::fs::write(dir.path().join("big.json"), big).unwrap();
    assert!(matches!(load(dir.path(), "big"), Err(AppError::Validation(_))));
}

#[test]
fn read_text_capped_returns_exact_text_and_caps_oversize() {
    let dir = tempdir().unwrap();
    let small = dir.path().join("a.csv");
    std::fs::write(&small, "id,prompt\n1,hello\n").unwrap();
    assert_eq!(read_text_capped(&small).unwrap(), "id,prompt\n1,hello\n");

    let big = dir.path().join("big.csv");
    std::fs::write(&big, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
    assert!(matches!(read_text_capped(&big), Err(AppError::Validation(_))));
}

#[test]
fn save_invalid_tasks_rejected() {
    let dir = tempdir().unwrap();
    assert!(matches!(save(dir.path(), "empty", &[]), Err(AppError::InvalidTaskSchema(_))));
}

fn broken_world_state_tasks() -> Vec<ToolTask> {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    // Structurally valid, semantically broken: an orphaned entity id no prompt or
    // blob names — the exact defect class the write boundary must block.
    let mut tasks = load_v2_collection(v2_json("hard-support-ecommerce").unwrap()).unwrap();
    let ws = tasks[0].agentic.as_mut().unwrap().world_state.as_mut().unwrap();
    ws.as_object_mut().unwrap().insert("Z-99".into(), json!({ "status": "lost" }));
    tasks
}

#[test]
fn save_rejects_semantically_broken_agentic_tasks() {
    let dir = tempdir().unwrap();
    let tasks = broken_world_state_tasks();
    let err = save(dir.path(), "broken_ws", &tasks).unwrap_err();
    let AppError::InvalidTaskSchema(msg) = err else { panic!("expected InvalidTaskSchema, got {err:?}") };
    assert!(msg.contains("Z-99"), "error names the broken key: {msg}");
    assert!(msg.contains(&tasks[0].id), "error names the task: {msg}");
    assert!(list(dir.path()).unwrap().is_empty(), "nothing may be written on rejection");
}

#[test]
fn save_allows_warning_only_findings() {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    use crate::inference::eval::agentic::sandbox::EndStateRule;
    // An UNGROUNDED answer token is a Warning-severity heuristic: it must surface
    // through the validate paths, never block the write (the author disposes).
    let dir = tempdir().unwrap();
    let mut tasks = load_v2_collection(v2_json("hard-support-ecommerce").unwrap()).unwrap();
    let spec = tasks[0].agentic.as_mut().unwrap();
    let EndStateRule::RequireAll(cps) = &mut spec.end_state else { panic!("fixture is RequireAll") };
    let cp = cps.iter_mut().find(|c| c.tool == "log_decision").unwrap();
    cp.args["decision"] = json!("*xylophone_zebra*");
    save(dir.path(), "warned", &tasks).expect("warning-severity findings must not block a save");
    assert_eq!(list(dir.path()).unwrap(), vec!["warned".to_string()]);
}

#[test]
fn load_still_reads_a_preexisting_broken_file() {
    // A file that predates the write boundary (or was hand-edited) must stay
    // loadable so the user can open and FIX it — enforcement is write-side only.
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    let tasks = broken_world_state_tasks();
    std::fs::write(dir.path().join("legacy.json"), serde_json::to_string_pretty(&tasks).unwrap()).unwrap();
    let loaded = load(dir.path(), "legacy").unwrap();
    assert_eq!(loaded.len(), tasks.len());
}

#[test]
fn bad_names_rejected() {
    let dir = tempdir().unwrap();
    for name in ["../escape", "a/b", "", "..", ".hidden"] {
        assert!(sanitize_name(name).is_err(), "should reject {name:?}");
        assert!(load(dir.path(), name).is_err());
    }
}

#[test]
fn parse_collection_loads_a_v2_object_collection() {
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    // A v2 collection (top-level OBJECT) transpiles via load_v2_collection — the env activates.
    let tasks = parse_collection(v2_json("easy-webui-tasks").unwrap()).unwrap();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().all(|t| t.category == "agent_loop"));
}

#[test]
fn parse_collection_loads_a_raw_task_array() {
    // A raw ToolTask[] (top-level ARRAY) takes the legacy path.
    let arr = serde_json::to_string(&sample()).unwrap();
    assert_eq!(parse_collection(&arr).unwrap().len(), 1);
}

#[test]
fn parse_collection_object_missing_v2_fields_errors_not_raw_fallthrough() {
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    // A top-level object that LOOKS v2-ish but is invalid (strip required `tier`) must return a
    // clear v2 error — NEVER silently fall through to the raw-array path.
    let mut v: serde_json::Value = serde_json::from_str(v2_json("easy-webui-tasks").unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("tier");
    assert!(parse_collection(&v.to_string()).is_err());
}

#[test]
fn parse_collection_rejects_non_object_non_array_and_non_json() {
    assert!(parse_collection("\"a string\"").is_err());
    assert!(parse_collection("42").is_err());
    assert!(parse_collection("{ not json").is_err());
}

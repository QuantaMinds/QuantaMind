use super::*;
use crate::inference::eval::agentic::v2::oracle::TaskValidation;

fn tv(id: &str) -> TaskValidation {
    TaskValidation {
        id: id.into(),
        reachable: "yes".into(),
        discriminating: Some(true),
        detail: String::new(),
        semantic: vec![],
        semantic_warnings: vec![],
    }
}

#[test]
fn exit_mapping_clean_warning_invalid() {
    let clean = CollectionValidation { ok: true, structural_error: None, tasks: vec![tv("a")] };
    assert_eq!(validate_exit(&clean), 0);

    let mut warn = clean.clone();
    warn.tasks[0].semantic_warnings.push("w".into());
    assert_eq!(validate_exit(&warn), 10);

    let invalid = CollectionValidation { ok: false, structural_error: None, tasks: vec![tv("a")] };
    assert_eq!(validate_exit(&invalid), 20);
}

#[test]
fn world_file_shape_is_detected_and_built() {
    // The friendly authoring shape (instruction + world) → build_mcp_tasks route.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("world.json");
    std::fs::write(
        &p,
        r#"[{"name":"summarize","instruction":"Read notes.txt and write summary.md",
             "world":{"type":"fs","files":[{"path":"notes.txt","content":"alpha"}]},
             "oracle":{"assert_present":["summary.md"]}}]"#,
    )
    .unwrap();
    let loaded = crate::cli::run::load_collection(p.to_str().unwrap()).expect("world file loads");
    assert!(loaded.from_file);
    assert_eq!(loaded.tasks.len(), 1);
    let mcp = loaded.tasks[0].agentic.as_ref().and_then(|a| a.mcp.as_ref());
    assert!(mcp.is_some(), "world file must produce an MCP-backed task");
}

#[test]
fn a_plain_tooltask_array_is_not_misdetected_as_a_world_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("plain.json");
    std::fs::write(
        &p,
        r#"[{"id":"t1","category":"single","prompt":"Weather in Tokyo? Use the tool.","description":"d",
             "tools":[{"name":"get_weather","description":"d","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}],
             "expected":{"type":"call","name":"get_weather","args":{"city":"Tokyo"}}}]"#,
    )
    .unwrap();
    let loaded = crate::cli::run::load_collection(p.to_str().unwrap()).expect("plain array loads");
    assert!(loaded.from_file);
    assert!(loaded.tasks[0].agentic.is_none(), "no phantom world on a plain task");
}

#[test]
fn builtin_ids_are_not_from_file() {
    let loaded = crate::cli::run::load_collection("easy-coding").expect("builtin loads");
    assert!(!loaded.from_file, "built-ins skip the upload gate (CI-guarded at authoring time)");
}

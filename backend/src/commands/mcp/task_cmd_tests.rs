use super::*;
use crate::commands::mcp::run_cmd::{FileSpec, McpTaskSpec, OracleSpec, WorldSpec};
use crate::inference::eval::mcp::world::McpSpec;
use crate::inference::eval::toolcall::tasks::validate_tasks;

#[test]
fn fs_task_converts_validates_and_asks_the_model_to_discover_the_dir() {
    let spec = McpTaskSpec {
        name: "Create result".into(),
        instruction: "create result.txt containing DONE".into(),
        world: WorldSpec::Fs { files: vec![FileSpec { path: "keep.txt".into(), content: "keep".into() }] },
        oracle: OracleSpec {
            assert_present: vec!["result.txt".into()],
            assert_content: vec![("result.txt".into(), "DONE".into())],
            ..Default::default()
        },
        k: 3,
    };
    let t = to_tooltask(&spec, 0);

    assert_eq!(t.category, "agent_loop");
    assert!(t.tools.iter().any(|s| s.name == "write_file"));
    assert!(t.tools.iter().any(|s| s.name == "list_allowed_directories"));
    assert!(t.prompt.contains("list_allowed_directories"), "prompt must tell the model to discover its dir");

    let ag = t.agentic.as_ref().unwrap();
    assert!(matches!(ag.end_state, EndStateRule::RequireWorldOracle));
    assert!(matches!(ag.environment, EnvKind::Mcp));
    assert!(matches!(ag.mcp, Some(McpSpec::Fs { .. })));

    // Passes the same trust boundary as a custom collection.
    validate_tasks(&[t]).unwrap();
}

#[test]
fn db_task_converts_and_validates() {
    let spec = McpTaskSpec {
        name: "insert alice".into(),
        instruction: "insert a row for Alice".into(),
        world: WorldSpec::Db { setup_sql: "CREATE TABLE users(name TEXT);".into() },
        oracle: OracleSpec {
            assert_eq: vec![("SELECT COUNT(*) FROM users WHERE name='Alice'".into(), "1".into())],
            ..Default::default()
        },
        k: 2,
    };
    let t = to_tooltask(&spec, 1);
    assert!(t.tools.iter().any(|s| s.name == "write_query"));
    assert!(matches!(t.agentic.as_ref().unwrap().mcp, Some(McpSpec::Db { .. })));
    validate_tasks(&[t]).unwrap();
}

#[test]
fn every_bundled_fs_schema_is_a_valid_object_schema() {
    // If any fixture tool's inputSchema weren't a valid object schema, validate_tasks would
    // reject the whole task — so a green fs conversion above already proves it, but assert the
    // count too (the reference server exposes 14).
    let spec = McpTaskSpec {
        name: "x".into(),
        instruction: "y".into(),
        world: WorldSpec::Fs { files: vec![] },
        oracle: OracleSpec::default(),
        k: 1,
    };
    assert_eq!(to_tooltask(&spec, 0).tools.len(), 14);
}

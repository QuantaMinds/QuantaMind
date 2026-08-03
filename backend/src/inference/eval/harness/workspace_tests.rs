use super::*;
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::{DbSeed, FsSeed};

fn fs_spec(seed: FsSeed) -> McpSpec {
    McpSpec::Fs { seed, oracle: FsOracle { assert_present: vec!["x".into()], ..Default::default() } }
}

#[test]
fn the_seed_lands_in_the_graded_root_not_the_scratch_parent() {
    let ws = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "alpha")])), "t", "g").unwrap();
    assert_eq!(std::fs::read_to_string(ws.root().join("a.txt")).unwrap(), "alpha");
    // The parent holds our files; the root must hold only the world.
    assert!(!ws.root().join("task.json").exists());
}

#[test]
fn nested_seed_paths_are_created() {
    let ws = Workspace::seed(&fs_spec(FsSeed::from([("deep/nested/f.txt", "v")])), "t", "g").unwrap();
    assert_eq!(std::fs::read_to_string(ws.root().join("deep/nested/f.txt")).unwrap(), "v");
}

/// The graded root must contain exactly the seed — nothing of ours — or "end
/// state" would mean "the seed, the agent's work, AND harness artifacts".
#[test]
fn task_file_and_otel_dir_are_outside_the_graded_root() {
    let ws = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "x")])), "t", "g").unwrap();
    assert!(!ws.task_file().starts_with(ws.root()));
    assert!(!ws.otel_dir().starts_with(ws.root()));
    assert!(ws.task_file().exists() && ws.otel_dir().is_dir());

    let entries: Vec<String> = std::fs::read_dir(ws.root())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["a.txt".to_string()], "graded root must hold ONLY the seed");
}

#[test]
fn the_task_file_carries_the_id_and_goal_the_agent_needs() {
    let ws = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "x")])), "task-7", "Refund order 4471").unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.task_file()).unwrap()).unwrap();
    assert_eq!(v["id"], "task-7");
    assert_eq!(v["goal"], "Refund order 4471");
}

/// The goal is handed over verbatim. The MCP path wraps instructions with
/// "FIRST call `list_allowed_directories`…" — an instruction about OUR reference
/// server that would be nonsense to somebody else's agent.
#[test]
fn the_goal_is_verbatim_with_no_harness_preamble() {
    let goal = "Delete tickets/T-1041.md";
    let ws = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "x")])), "t", goal).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(ws.task_file()).unwrap()).unwrap();
    assert_eq!(v["goal"].as_str().unwrap(), goal);
    assert!(!v["goal"].as_str().unwrap().contains("list_allowed_directories"));
}

/// A malformed seed must be an error, not a panic and not a silent escape.
#[test]
fn an_escaping_seed_path_errors_rather_than_writing_outside() {
    for bad in ["../escape.txt", "/etc/escape"] {
        let r = Workspace::seed(&fs_spec(FsSeed::from([(bad, "x")])), "t", "g");
        assert!(r.is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn two_workspaces_never_share_a_root() {
    let a = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "1")])), "t", "g").unwrap();
    let b = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "2")])), "t", "g").unwrap();
    assert_ne!(a.root(), b.root());
    assert_eq!(std::fs::read_to_string(a.root().join("a.txt")).unwrap(), "1");
    assert_eq!(std::fs::read_to_string(b.root().join("a.txt")).unwrap(), "2");
}

#[test]
fn the_whole_tree_is_removed_on_drop() {
    let (root, parent) = {
        let ws = Workspace::seed(&fs_spec(FsSeed::from([("a.txt", "x")])), "t", "g").unwrap();
        (ws.root().to_path_buf(), ws.task_file().parent().unwrap().to_path_buf())
    };
    assert!(!root.exists(), "graded root removed");
    assert!(!parent.exists(), "scratch parent removed too — no leak");
}

#[test]
#[ignore = "needs the sqlite3 CLI on PATH"]
fn a_db_world_materializes_the_database_inside_the_graded_root() {
    let spec = McpSpec::Db {
        seed: DbSeed::new("CREATE TABLE t(a INT); INSERT INTO t VALUES (1);"),
        oracle: DbOracle { assert_eq: vec![("SELECT count(*) FROM t;".into(), "1".into())], ..Default::default() },
    };
    let ws = Workspace::seed(&spec, "t", "g").unwrap();
    let db = ws.db().expect("db world must expose its file");
    assert!(db.exists(), "the database file must exist");
    assert!(db.starts_with(ws.root()), "the db is world state — it must be inside the graded root");
}

#[test]
#[ignore = "needs the sqlite3 CLI on PATH"]
fn an_empty_setup_sql_still_produces_a_database() {
    // "the agent creates the schema" is a legitimate world; sqlite still needs a
    // statement to materialize the file.
    let spec = McpSpec::Db {
        seed: DbSeed::new(""),
        oracle: DbOracle { assert_eq: vec![("SELECT 1;".into(), "1".into())], ..Default::default() },
    };
    let ws = Workspace::seed(&spec, "t", "g").unwrap();
    assert!(ws.db().unwrap().exists());
}

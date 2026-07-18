use super::*;
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::{DbSeed, FsSeed};

fn fs_spec(seed: FsSeed, oracle: FsOracle) -> McpSpec {
    McpSpec::Fs { seed, oracle }
}

#[test]
fn a_sound_fs_world_has_no_static_findings() {
    let spec = fs_spec(
        FsSeed::from([("notes.txt", "alpha")]),
        FsOracle { assert_present: vec!["summary.md".into()], ..Default::default() },
    );
    assert!(static_world_findings(&spec).is_empty());
}

#[test]
fn a_vacuous_fs_oracle_is_an_error_finding() {
    // No assertions at all → every run passes no matter what the model does.
    let spec = fs_spec(FsSeed::from([("notes.txt", "alpha")]), FsOracle::default());
    let f = static_world_findings(&spec);
    assert_eq!(f.len(), 1);
    assert!(f[0].contains("vacuous oracle"), "{f:?}");
}

#[test]
fn contradictory_present_and_absent_is_flagged() {
    let spec = fs_spec(
        FsSeed::default(),
        FsOracle {
            assert_present: vec!["out.txt".into()],
            assert_absent: vec!["out.txt".into()],
            ..Default::default()
        },
    );
    let f = static_world_findings(&spec);
    assert!(f.iter().any(|m| m.contains("unsatisfiable")), "{f:?}");
}

#[test]
fn content_on_an_absent_path_is_flagged() {
    let spec = fs_spec(
        FsSeed::default(),
        FsOracle {
            assert_absent: vec!["gone.txt".into()],
            assert_content: vec![("gone.txt".into(), "x".into())],
            ..Default::default()
        },
    );
    let f = static_world_findings(&spec);
    assert!(f.iter().any(|m| m.contains("also asserted absent")), "{f:?}");
}

#[test]
fn escaping_and_absolute_seed_paths_are_flagged_and_redacted() {
    let spec = fs_spec(
        FsSeed::from([("../escape.txt", "x"), ("/Users/alice/abs.txt", "y")]),
        FsOracle { assert_present: vec!["ok.txt".into()], ..Default::default() },
    );
    let f = static_world_findings(&spec);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f.iter().all(|m| m.contains("must be relative")));
    // rule 7f: the username from the absolute path never survives into a finding.
    assert!(f.iter().all(|m| !m.contains("alice")), "{f:?}");
}

#[test]
fn a_db_world_with_empty_seed_is_fine_but_vacuous_oracle_is_not() {
    // "Model creates the schema" is a legitimate world → empty setup_sql is OK...
    let ok = McpSpec::Db {
        seed: DbSeed::default(),
        oracle: DbOracle { assert_contains: vec![("SELECT name FROM users".into(), "alice".into())], ..Default::default() },
    };
    assert!(static_world_findings(&ok).is_empty());
    // ...but an oracle asserting nothing is vacuous.
    let vac = McpSpec::Db { seed: DbSeed::new("CREATE TABLE t(x);"), oracle: DbOracle::default() };
    assert!(static_world_findings(&vac)[0].contains("vacuous oracle"));
}

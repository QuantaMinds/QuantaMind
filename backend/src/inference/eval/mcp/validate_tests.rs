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
    // Traversal, a POSIX root, and a Windows drive prefix — all must be caught on
    // EVERY platform (a Unix `/Users/...` is not `is_absolute()` on Windows, so the
    // check can't lean on that alone).
    let spec = fs_spec(
        FsSeed::from([("../escape.txt", "x"), ("/Users/alice/abs.txt", "y"), ("C:\\Users\\bob\\abs.txt", "z")]),
        FsOracle { assert_present: vec!["ok.txt".into()], ..Default::default() },
    );
    let f = static_world_findings(&spec);
    assert_eq!(f.len(), 3, "all three unsafe seeds flagged on any platform: {f:?}");
    assert!(f.iter().all(|m| m.contains("must be relative")));
    // rule 7f: the username from an absolute path never survives into a finding.
    assert!(f.iter().all(|m| !m.contains("alice") && !m.contains("bob")), "{f:?}");
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

/// THE HOLE. `static_world_findings` guarded only *seed* paths, and
/// `FsOracle::grade` does a raw `root.join(p)` — where `Path::join` with an
/// ABSOLUTE path discards the root entirely. So an oracle asserting
/// `/etc/passwd` present passed on every Unix machine regardless of what the
/// agent did: a vacuous assertion that silently reads the host filesystem
/// instead of the sandbox. `..` traversal escapes the same way.
#[test]
fn an_oracle_path_that_escapes_the_sandbox_is_an_error() {
    for escape in ["/etc/passwd", "../../../../etc/passwd", "..\\..\\windows\\system32"] {
        let spec = fs_spec(
            FsSeed::from([("notes.txt", "alpha")]),
            FsOracle { assert_present: vec![escape.into()], ..Default::default() },
        );
        let f = static_world_findings(&spec);
        assert!(
            f.iter().any(|m| m.contains("escape")),
            "oracle path {escape:?} must be rejected, got {f:?}"
        );
    }
}

#[test]
fn every_fs_oracle_field_is_path_checked_not_just_assert_present() {
    let absent = fs_spec(
        FsSeed::from([("a.txt", "x")]),
        FsOracle { assert_absent: vec!["/etc/hosts".into()], ..Default::default() },
    );
    assert!(static_world_findings(&absent).iter().any(|m| m.contains("escape")), "assert_absent");

    let content = fs_spec(
        FsSeed::from([("a.txt", "x")]),
        FsOracle { assert_content: vec![("/etc/hosts".into(), "root".into())], ..Default::default() },
    );
    assert!(static_world_findings(&content).iter().any(|m| m.contains("escape")), "assert_content");
}

/// A "SELECT" that is really an UPDATE would make the oracle self-fulfilling:
/// grading would CREATE the state it claims to be checking. Warned, not errored,
/// because a legitimate read can start with a CTE or a comment.
#[test]
fn a_non_select_db_oracle_query_is_flagged() {
    let spec = McpSpec::Db {
        seed: DbSeed { setup_sql: "CREATE TABLE t(a INT);".into() },
        oracle: DbOracle {
            assert_eq: vec![("INSERT INTO t VALUES (1); SELECT count(*) FROM t;".into(), "1".into())],
            ..Default::default()
        },
    };
    let f = static_world_findings(&spec);
    assert!(f.iter().any(|m| m.contains("SELECT")), "a mutating oracle query must be flagged: {f:?}");
}

/// The redaction rule (7f): an absolute oracle path is exactly the case that can
/// carry /Users/<name>/… — the finding must never echo it verbatim.
#[test]
fn an_escaping_oracle_path_is_redacted_in_the_finding() {
    let spec = fs_spec(
        FsSeed::from([("a.txt", "x")]),
        FsOracle { assert_present: vec!["/Users/someone/secret/notes.txt".into()], ..Default::default() },
    );
    let f = static_world_findings(&spec);
    assert!(!f.iter().any(|m| m.contains("/Users/someone")), "must be redacted: {f:?}");
}

/// Positive control for the read-only predicate: it must not reject legitimate
/// reads. A gate that flags valid worlds gets switched off, which is worse than
/// no gate.
#[test]
fn legitimate_read_queries_are_not_flagged() {
    for q in [
        "SELECT count(*) FROM t",
        "select count(*) from t",                       // lowercase
        "  \n\t SELECT 1",                              // leading whitespace
        "-- a note\nSELECT 1",                          // line comment
        "/* a note */ SELECT 1",                        // block comment
        "/* one */ -- two\n  SELECT 1",                 // stacked comments
        "WITH x AS (SELECT 1) SELECT * FROM x",         // CTE
        "with x as (select 1) select * from x",
    ] {
        let spec = McpSpec::Db {
            seed: DbSeed { setup_sql: "CREATE TABLE t(a INT);".into() },
            oracle: DbOracle { assert_eq: vec![(q.into(), "1".into())], ..Default::default() },
        };
        let f = static_world_findings(&spec);
        assert!(!f.iter().any(|m| m.contains("must be a read")), "{q:?} is a read, got {f:?}");
    }
}

#[test]
fn mutating_oracle_queries_are_flagged_whatever_the_verb() {
    for q in ["INSERT INTO t VALUES (1)", "update t set a=1", "DELETE FROM t", "DROP TABLE t", "PRAGMA foo"] {
        let spec = McpSpec::Db {
            seed: DbSeed { setup_sql: "CREATE TABLE t(a INT);".into() },
            oracle: DbOracle { assert_contains: vec![(q.into(), "1".into())], ..Default::default() },
        };
        assert!(
            static_world_findings(&spec).iter().any(|m| m.contains("must be a read")),
            "{q:?} mutates and must be flagged"
        );
    }
}

/// A relative oracle path — the normal case — must stay clean, or every existing
/// world would start failing validation.
#[test]
fn ordinary_relative_oracle_paths_are_untouched() {
    let spec = fs_spec(
        FsSeed::from([("in.txt", "x")]),
        FsOracle {
            assert_present: vec!["out/summary.md".into(), "deep/nested/file.txt".into()],
            assert_absent: vec!["in.txt".into()],
            assert_content: vec![("out/summary.md".into(), "DONE".into())],
        },
    );
    assert!(static_world_findings(&spec).is_empty(), "{:?}", static_world_findings(&spec));
}

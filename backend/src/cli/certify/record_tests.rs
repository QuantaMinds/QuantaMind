use super::*;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::{DbSeed, FsSeed};
use crate::os::ScratchDir;

fn snap(pairs: &[(&str, &str)]) -> Snapshot {
    pairs.iter().map(|(p, c)| (p.to_string(), c.as_bytes().to_vec())).collect()
}

#[test]
fn a_new_file_is_recorded_as_created() {
    let d = diff(&snap(&[("in.txt", "a")]), &snap(&[("in.txt", "a"), ("out.txt", "b")]));
    assert_eq!(d.created, vec!["out.txt"]);
    assert!(d.deleted.is_empty() && d.modified.is_empty());
}

#[test]
fn a_removed_file_is_recorded_as_deleted() {
    let d = diff(&snap(&[("stale.log", "x")]), &snap(&[]));
    assert_eq!(d.deleted, vec!["stale.log"]);
    assert!(d.created.is_empty());
}

/// A changed body is reported but NOT auto-asserted: a recorded body would embed
/// timestamps and ids and fail on its own second run.
#[test]
fn a_changed_file_is_reported_as_modified_and_never_asserted() {
    let d = diff(&snap(&[("report.md", "status: open")]), &snap(&[("report.md", "status: done")]));
    assert_eq!(d.modified, vec!["report.md"]);
    let o = to_oracle_json(&d);
    assert_eq!(o["assert_present"].as_array().unwrap().len(), 0);
    assert_eq!(o["assert_absent"].as_array().unwrap().len(), 0);
}

#[test]
fn an_untouched_world_records_nothing_and_says_so() {
    let d = diff(&snap(&[("a.txt", "x")]), &snap(&[("a.txt", "x")]));
    assert!(d.is_empty(), "a recording of nothing cannot become a test");
}

/// `is_empty` must ignore `modified`: a run that only edited a file in place
/// yields no present/absent assertion, so the oracle would be vacuous.
#[test]
fn a_modify_only_run_still_counts_as_nothing_recorded() {
    let d = diff(&snap(&[("a.txt", "x")]), &snap(&[("a.txt", "y")]));
    assert_eq!(d.modified, vec!["a.txt"]);
    assert!(d.is_empty(), "modify-only yields no assertion, so it is not a recordable test");
}

#[test]
fn the_recorded_oracle_is_exactly_the_structural_delta() {
    let d = diff(
        &snap(&[("tickets/T-1.md", "open")]),
        &snap(&[("out/summary.md", "RESOLVED")]),
    );
    let o = to_oracle_json(&d);
    assert_eq!(o["assert_present"][0], "out/summary.md");
    assert_eq!(o["assert_absent"][0], "tickets/T-1.md");
}

#[test]
fn snapshot_walks_nested_directories_with_forward_slashes() {
    let dir = ScratchDir::new("qm-record-test").unwrap();
    std::fs::create_dir_all(dir.path().join("out/deep")).unwrap();
    std::fs::write(dir.path().join("out/deep/f.txt"), "v").unwrap();
    std::fs::write(dir.path().join("top.txt"), "t").unwrap();
    let s = snapshot(dir.path());
    assert_eq!(s.get("out/deep/f.txt").map(|v| v.as_slice()), Some(b"v".as_slice()));
    assert!(s.contains_key("top.txt"));
    assert!(!s.keys().any(|k| k.contains('\\')), "paths must be portable: {:?}", s.keys());
}

/// A recorded suite must describe the world, not wherever a link points on the
/// machine that recorded it.
#[cfg(unix)]
#[test]
fn snapshot_does_not_follow_symlinks() {
    let dir = ScratchDir::new("qm-record-test").unwrap();
    std::fs::write(dir.path().join("real.txt"), "r").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("link.txt")).unwrap();
    let s = snapshot(dir.path());
    assert!(s.contains_key("real.txt"));
    assert!(!s.contains_key("link.txt"), "a symlink must not be captured");
}

#[test]
fn the_seed_is_read_in_the_same_terms_as_the_end_state() {
    let spec = McpSpec::Fs {
        seed: FsSeed::from([("a.txt", "alpha")]),
        oracle: FsOracle::default(),
    };
    let s = seed_snapshot(&spec);
    assert_eq!(s.get("a.txt").map(|v| v.as_slice()), Some(b"alpha".as_slice()));
}

/// Emitting a plausible-looking SQL oracle we never verified would be worse than
/// emitting nothing, so db worlds are refused with a reason rather than guessed.
#[test]
fn db_worlds_are_refused_with_a_reason_not_guessed() {
    let db = McpSpec::Db { seed: DbSeed::new("CREATE TABLE t(a);"), oracle: Default::default() };
    let why = unsupported_reason(&db).expect("db must be refused");
    assert!(why.contains("schema-aware"), "{why}");
    assert!(unsupported_reason(&McpSpec::Fs {
        seed: FsSeed::from([("a", "b")]),
        oracle: FsOracle::default()
    })
    .is_none());
}

#[test]
fn the_review_banner_warns_about_real_data() {
    assert!(REVIEW_BANNER.contains("real data"));
    assert!(REVIEW_BANNER.contains("review before committing"));
}

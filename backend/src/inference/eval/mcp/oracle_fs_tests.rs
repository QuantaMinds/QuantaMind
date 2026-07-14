use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// A throwaway dir (server-free) for grading tests.
fn scratch() -> std::path::PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir()
        .join(format!("qm-fsoracle-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn present_absent_and_content_all_pass() {
    let root = scratch();
    std::fs::write(root.join("result.txt"), "the answer is DONE").unwrap();
    let oracle = FsOracle {
        assert_present: vec!["result.txt".into()],
        assert_absent: vec!["ghost.txt".into()],
        assert_content: vec![("result.txt".into(), "DONE".into())],
    };
    let v = oracle.grade(&root);
    assert!(v.passed, "failures: {:?}", v.failures);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_present_file_fails() {
    let root = scratch();
    let oracle = FsOracle { assert_present: vec!["result.txt".into()], ..Default::default() };
    let v = oracle.grade(&root);
    assert!(!v.passed);
    assert!(v.failures[0].contains("result.txt"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_file_that_should_be_absent_but_exists_fails() {
    let root = scratch();
    std::fs::write(root.join("old.log"), "x").unwrap();
    let oracle = FsOracle { assert_absent: vec!["old.log".into()], ..Default::default() };
    let v = oracle.grade(&root);
    assert!(!v.passed);
    assert!(v.failures[0].contains("old.log"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn wrong_content_fails() {
    let root = scratch();
    std::fs::write(root.join("result.txt"), "nope").unwrap();
    let oracle = FsOracle {
        assert_content: vec![("result.txt".into(), "DONE".into())],
        ..Default::default()
    };
    let v = oracle.grade(&root);
    assert!(!v.passed);
    assert!(v.failures[0].contains("DONE"));
    std::fs::remove_dir_all(&root).ok();
}

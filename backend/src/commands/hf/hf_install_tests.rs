use super::*;
use std::io::Write;

#[test]
fn cleanup_removes_both_the_partial_and_the_dest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("model.gguf");
    let partial = partial_path(&dest);
    std::fs::File::create(&dest).unwrap().write_all(b"x").unwrap();
    std::fs::File::create(&partial).unwrap().write_all(b"y").unwrap();

    cleanup_incomplete_download(&dest);

    assert!(!dest.exists(), "dest should be removed");
    assert!(!partial.exists(), "partial should be removed");
}

#[test]
fn cleanup_is_idempotent_when_nothing_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    // No files created — must not panic or error.
    cleanup_incomplete_download(&dir.path().join("absent.gguf"));
}

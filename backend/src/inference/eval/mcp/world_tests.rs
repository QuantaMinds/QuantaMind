//! Pure seed-writing tests (no server). The full seed→execute→end-state→teardown
//! is a live test (`tests/mcp_live.rs`, `--ignored`).

use super::*;

/// Same self-cleaning scratch dir the world uses, for a server-free seed test.
fn scratch() -> ScratchDir {
    ScratchDir::new().unwrap()
}

#[test]
fn write_seed_creates_files_and_nested_dirs() {
    let dir = scratch();
    let seed = FsSeed::from([("old.log", "stale"), ("sub/deep.txt", "nested")]);
    write_seed(&dir.path, &seed).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path.join("old.log")).unwrap(), "stale");
    assert_eq!(std::fs::read_to_string(dir.path.join("sub/deep.txt")).unwrap(), "nested");
}

#[test]
fn write_seed_rejects_escape_paths() {
    let dir = scratch();
    let esc = FsSeed::from([("../evil.txt", "x")]);
    assert!(write_seed(&dir.path, &esc).is_err(), ".. is rejected");
    let abs = FsSeed::from([("/etc/evil", "x")]);
    assert!(write_seed(&dir.path, &abs).is_err(), "absolute is rejected");
}

#[test]
fn scratch_dir_is_unique_and_removed_on_drop() {
    let (p1, p2);
    {
        let a = scratch();
        let b = scratch();
        p1 = a.path.clone();
        p2 = b.path.clone();
        assert_ne!(p1, p2, "each scratch dir is distinct (fresh per run)");
        assert!(p1.exists() && p2.exists());
    }
    assert!(!p1.exists() && !p2.exists(), "scratch dirs removed on drop");
}

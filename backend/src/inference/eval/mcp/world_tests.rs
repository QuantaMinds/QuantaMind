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
#[cfg(unix)]
fn orphan_dirs_of_dead_pids_are_swept_but_live_ones_kept() {
    // A SIGKILL'd run can't Drop — simulate its leak with a dir under a pid that
    // cannot be alive (beyond macOS/Linux pid ranges), plus one owned by US.
    let tmp = std::env::temp_dir();
    let dead = tmp.join("qm-mcp-world-4194000-99");
    let mine = tmp.join(format!("qm-mcp-world-{}-777", std::process::id()));
    std::fs::create_dir_all(&dead).unwrap();
    std::fs::create_dir_all(&mine).unwrap();

    let _s = scratch(); // ScratchDir::new runs the sweep

    assert!(!dead.exists(), "a dead pid's leaked world dir must be swept");
    assert!(mine.exists(), "our own live world dirs must never be swept");
    let _ = std::fs::remove_dir_all(&mine);
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

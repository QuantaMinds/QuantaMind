use super::*;

const T: &str = "qm-scratch-test";

#[test]
fn creates_a_real_dir_and_removes_it_on_drop() {
    let path = {
        let s = ScratchDir::new(T).expect("scratch dir");
        assert!(s.path().is_dir(), "dir must exist while alive");
        s.path().to_path_buf()
    };
    assert!(!path.exists(), "dir must be gone after drop");
}

#[test]
fn two_dirs_from_one_process_never_collide() {
    let a = ScratchDir::new(T).unwrap();
    let b = ScratchDir::new(T).unwrap();
    assert_ne!(a.path(), b.path(), "counter must make each dir unique");
    assert!(a.path().is_dir() && b.path().is_dir());
}

#[test]
fn the_name_carries_the_prefix_and_our_pid() {
    let s = ScratchDir::new(T).unwrap();
    let name = s.path().file_name().unwrap().to_str().unwrap().to_string();
    assert!(name.starts_with(&format!("{T}-")), "{name}");
    assert!(
        name.starts_with(&format!("{T}-{}-", std::process::id())),
        "the pid must be in the name or the sweep can't tell whose dir it is: {name}"
    );
}

#[test]
fn a_different_prefix_is_a_different_namespace() {
    let a = ScratchDir::new("qm-scratch-test-alpha").unwrap();
    let b = ScratchDir::new("qm-scratch-test-beta").unwrap();
    // Sweeping one prefix must never consider the other's directories.
    assert!(a.path().is_dir() && b.path().is_dir());
    drop(a);
    assert!(b.path().is_dir(), "dropping one prefix must not touch another");
}

#[test]
fn sweeps_a_dead_pids_orphan_but_never_a_live_ones() {
    // A dir owned by a pid that cannot be alive: u32::MAX is not a valid live pid
    // on any platform we build for.
    let orphan = std::env::temp_dir().join(format!("{T}-{}-0", u32::MAX));
    std::fs::create_dir_all(&orphan).unwrap();

    // A dir owned by a pid that definitely IS alive: our own. `sweep_orphans` skips
    // our pid explicitly, which is what keeps concurrent runs safe.
    let live = std::env::temp_dir().join(format!("{T}-{}-99999", std::process::id()));
    std::fs::create_dir_all(&live).unwrap();

    let _trigger = ScratchDir::new(T).unwrap(); // construction runs the sweep

    assert!(!orphan.exists(), "a dead pid's scratch dir must be reaped");
    assert!(live.exists(), "a live pid's scratch dir must never be reaped");
    let _ = std::fs::remove_dir_all(&live);
}

#[test]
fn sweep_ignores_names_it_does_not_own() {
    // Same temp dir, unrelated name → must survive untouched.
    let foreign = std::env::temp_dir().join("qm-scratch-test-UNRELATED-name");
    std::fs::create_dir_all(&foreign).unwrap();
    let _trigger = ScratchDir::new(T).unwrap();
    assert!(foreign.exists(), "a non-matching name must never be swept");
    let _ = std::fs::remove_dir_all(&foreign);
}

#[test]
fn a_malformed_pid_segment_is_skipped_not_parsed_loosely() {
    // `<prefix>-notapid-0` must not be swept — a loose parse could reap live dirs.
    let weird = std::env::temp_dir().join(format!("{T}-notapid-0"));
    std::fs::create_dir_all(&weird).unwrap();
    let _trigger = ScratchDir::new(T).unwrap();
    assert!(weird.exists(), "an unparseable pid must skip, not sweep");
    let _ = std::fs::remove_dir_all(&weird);
}

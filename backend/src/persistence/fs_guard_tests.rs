use super::*;
use std::os::unix::fs::symlink;
use tempfile::tempdir;

#[test]
fn accepts_child_path() {
    let root = tempdir().unwrap();
    let child = root.path().join("sub/x.quantamind.yaml");
    std::fs::create_dir_all(child.parent().unwrap()).unwrap();
    assert!(ensure_within(root.path(), &child).is_ok());
}

#[test]
fn accepts_new_file_that_does_not_exist_yet() {
    let root = tempdir().unwrap();
    let target = root.path().join("new.quantamind.yaml");
    // Parent (root) exists, file does not — the create case.
    let resolved = ensure_within(root.path(), &target).unwrap();
    assert!(resolved.starts_with(root.path().canonicalize().unwrap()));
}

#[test]
fn rejects_parent_outside_root() {
    let root = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let escape = elsewhere.path().join("x.quantamind.yaml");
    assert!(matches!(ensure_within(root.path(), &escape), Err(AppError::Validation(_))));
}

/// The core hardening: a symlink FILE inside the workspace whose target escapes root must be
/// rejected — the old parent-only check let this through and read/wrote the target.
#[test]
fn rejects_symlink_final_component_escaping_root() {
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, b"top secret").unwrap();

    let link = root.path().join("evil.quantamind.yaml");
    symlink(&secret, &link).unwrap();

    assert!(
        matches!(ensure_within(root.path(), &link), Err(AppError::Validation(_))),
        "a symlink escaping root must be rejected"
    );
}

/// A dangling symlink (points at a missing target) must not become a write-through primitive.
#[test]
fn rejects_dangling_symlink() {
    let root = tempdir().unwrap();
    let link = root.path().join("dangling.quantamind.yaml");
    symlink(root.path().join("does-not-exist"), &link).unwrap();
    assert!(ensure_within(root.path(), &link).is_err());
}

/// A symlink that stays WITHIN root is fine (canonicalizes to a path under root).
#[test]
fn accepts_symlink_pointing_inside_root() {
    let root = tempdir().unwrap();
    let real = root.path().join("real.quantamind.yaml");
    std::fs::write(&real, b"data").unwrap();
    let link = root.path().join("alias.quantamind.yaml");
    symlink(&real, &link).unwrap();
    let resolved = ensure_within(root.path(), &link).unwrap();
    assert_eq!(resolved, real.canonicalize().unwrap());
}

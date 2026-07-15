use super::*;
use crate::persistence::prompts::schema::{InferenceParams, PromptFile};
use tempfile::tempdir;

fn sample() -> PromptFile {
    PromptFile {
        name: "n".into(), system: "s".into(), user: "u".into(),
        model: Some("m".into()), params: InferenceParams::default(),
        created_at: "t".into(), updated_at: "t".into(), auto_rerun: false,
    }
}

#[test]
fn round_trip_preserves_file() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("a.quantamind.yaml");
    write(&p, &sample()).unwrap();
    assert_eq!(read(&p).unwrap(), sample());
}

#[test]
fn read_missing_returns_not_found() {
    let dir = tempdir().unwrap();
    match read(&dir.path().join("nope.yaml")) {
        Err(AppError::NotFound(_)) => (),
        other => panic!("expected NotFound, got {:?}", other),
    }
}

#[test]
fn write_creates_missing_parent_dir() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("nested/deep/a.quantamind.yaml");
    write(&p, &sample()).unwrap();
    assert!(p.exists());
}

#[test]
fn delete_removes_file() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("a.quantamind.yaml");
    write(&p, &sample()).unwrap();
    delete(&p).unwrap();
    assert!(!p.exists());
}

#[test]
fn delete_missing_returns_not_found() {
    let dir = tempdir().unwrap();
    match delete(&dir.path().join("missing")) {
        Err(AppError::NotFound(_)) => (),
        other => panic!("expected NotFound, got {:?}", other),
    }
}

#[test]
fn rename_succeeds_and_moves_file() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.quantamind.yaml");
    let b = dir.path().join("b.quantamind.yaml");
    write(&a, &sample()).unwrap();
    rename(&a, &b).unwrap();
    assert!(!a.exists() && b.exists());
}

#[test]
fn rename_to_existing_rejects() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.quantamind.yaml");
    let b = dir.path().join("b.quantamind.yaml");
    write(&a, &sample()).unwrap();
    write(&b, &sample()).unwrap();
    match rename(&a, &b) {
        Err(AppError::Validation(_)) => (),
        other => panic!("expected Validation, got {:?}", other),
    }
}
// `ensure_within` moved to `crate::fs_guard`; its tests live in `fs_guard_tests.rs`.

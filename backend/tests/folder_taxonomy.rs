//! The folder-taxonomy rule (docs/architecture.md#folder-taxonomy): no source
//! folder holds more than 10 `.rs` files; split into concern sub-folders at the
//! limit.
//!
//! KNOWN PRE-EXISTING DEBT (2026-06): four folders are over the limit —
//! `persistence/` (12), `inference/eval/toolcall/` (11), `commands/mlx/` (11),
//! `commands/llama/` (11). Splitting them is import-churny and is tracked as a
//! dedicated refactor (docs/restructure-todo.md), kept out of CI's required
//! checks so the dependency-law guard (`layering_guard.rs`) can gate PRs without
//! being blocked by this debt. Run `cargo test --test folder_taxonomy` to see
//! the current offenders; the goal is to drive this green.

use std::fs;
use std::path::Path;

#[test]
fn no_src_folder_exceeds_ten_rs_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    check_counts(&root, &mut offenders);
    assert!(
        offenders.is_empty(),
        "folders over the 10-file limit (see docs/architecture.md#folder-taxonomy): {offenders:?}"
    );
}

fn check_counts(dir: &Path, offenders: &mut Vec<String>) {
    let mut rs_here = 0;
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            check_counts(&path, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") {
            rs_here += 1;
        }
    }
    if rs_here > 10 {
        offenders.push(format!("{} ({rs_here} files)", dir.display()));
    }
}

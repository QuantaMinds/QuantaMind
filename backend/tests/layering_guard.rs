//! The dependency-direction law (docs/architecture.md#layering), mechanically
//! enforced. These tests are deterministic and infra-free, so CI gates them on
//! every PR (`cargo test --test layering_guard`). The separate folder-size rule
//! lives in `folder_taxonomy.rs` so this law-target stays independently green.

use std::fs;
use std::path::Path;

/// Enforces the layering law (docs/architecture.md#layering): the domain layer
/// `inference/` must not depend on the IPC layer `commands/`. If this
/// fails, a backward dependency crept back in — invert it via a sink/
/// callback trait instead of importing from `commands`.
#[test]
fn inference_does_not_import_commands() {
    assert_no_match(
        "src/inference",
        "use crate::commands",
        "inference/ must not import crate::commands — invert via a sink trait",
    );
}

/// The domain is a *driven adapter's dependency*, not the other way round:
/// `inference/` must not import `crate::persistence`. Persistence depends on
/// domain types (it serializes them), never the reverse — that one-way edge is
/// what makes the guide's litmus test ("domain compiles with infra deleted")
/// hold. If this fails, a persistence type leaked into the domain; move the
/// shared type into the domain instead (see `RunSummary` in inference/eval).
#[test]
fn inference_does_not_import_persistence() {
    assert_no_match(
        "src/inference",
        "use crate::persistence",
        "inference/ must not import crate::persistence — move the shared type into the domain",
    );
}

/// `inference/` is Tauri-free (docs/architecture.md#layering): it must name no
/// `tauri::` type. Progress crosses to the IPC layer via a sink trait, never an
/// `AppHandle`. Prevents the framework from leaking into the pure core.
#[test]
fn inference_does_not_name_tauri() {
    assert_no_match(
        "src/inference",
        "tauri::",
        "inference/ must not name any tauri:: type — take a sink trait, not an AppHandle",
    );
}

/// The leaf adapters `persistence/` and `metrics/` must not import the IPC layer
/// `commands/` — the edge points inward (commands -> domain -> adapters), never
/// back out to the driving adapter.
#[test]
fn leaves_do_not_import_commands() {
    assert_no_match(
        "src/persistence",
        "use crate::commands",
        "persistence/ must not import crate::commands",
    );
    assert_no_match(
        "src/metrics",
        "use crate::commands",
        "metrics/ must not import crate::commands",
    );
}

/// Scan every `.rs` under `rel` (a path relative to the crate root) and fail if
/// any file's body contains `needle`. Skips nothing — test files included, since
/// a leak in a test fixture is still a layering leak.
fn assert_no_match(rel: &str, needle: &str, law: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let mut offenders = Vec::new();
    visit(&dir, &mut |path, body| {
        if body.contains(needle) {
            offenders.push(path.display().to_string());
        }
    });
    assert!(
        offenders.is_empty(),
        "{law} (see docs/architecture.md#layering). Offenders: {offenders:?}"
    );
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let body = fs::read_to_string(&path).expect("read rs file");
            f(&path, &body);
        }
    }
}

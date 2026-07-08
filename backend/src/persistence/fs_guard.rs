//! fs_guard — the single confinement chokepoint for frontend-supplied paths (rule 7b).
//!
//! Every command that turns a path from the webview into a real filesystem operation inside a
//! workspace must resolve it through `ensure_within`, which guarantees the result stays under
//! `root` even when the final path component is a symlink. Consolidates what were two separate
//! copies of a weaker "canonicalize the parent only" check.

use crate::errors::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// Resolve `candidate` and guarantee it stays within `root`, rejecting `..` traversal AND
/// symlink escapes. `root` must exist. The candidate file itself need not exist yet (so this
/// also covers "create a new file" — its parent is confined and a not-yet-created name cannot
/// be a symlink). Returns the canonical path to use for the actual I/O.
pub fn ensure_within(root: &Path, candidate: &Path) -> AppResult<PathBuf> {
    let root_abs = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    let parent = candidate.parent().unwrap_or(Path::new("/"));
    let parent_abs = parent.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    if !parent_abs.starts_with(&root_abs) {
        return Err(escapes(candidate));
    }
    let name = candidate
        .file_name()
        .ok_or_else(|| AppError::Validation("missing file name".into()))?;
    let resolved = parent_abs.join(name);

    // If the final component already exists (regular file, dir, OR symlink — `symlink_metadata`
    // is an lstat that does not follow), canonicalize the WHOLE path so a symlink whose target
    // escapes `root` is caught. A dangling symlink lstats Ok but fails to canonicalize → also
    // rejected (a symlink in a workspace pointing at a missing target is not something we write
    // through). A path that does not exist at all → new file, already confined by the parent.
    match resolved.symlink_metadata() {
        Ok(_) => {
            let full = resolved.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
            if !full.starts_with(&root_abs) {
                return Err(escapes(candidate));
            }
            Ok(full)
        }
        Err(_) => Ok(resolved),
    }
}

fn escapes(candidate: &Path) -> AppError {
    AppError::Validation(format!("path escapes workspace: {}", candidate.display()))
}

#[cfg(test)]
#[path = "fs_guard_tests.rs"]
mod tests;

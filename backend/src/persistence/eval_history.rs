use crate::errors::{AppError, AppResult};
// `RunSummary` is a domain type owned by the eval engine; persistence is a driven
// adapter that depends on it (the edge points inward, never the reverse).
use crate::inference::eval::run_summary::RunSummary;
use crate::persistence::evals::sanitize_name;
use std::path::{Path, PathBuf};

/// Keep history files small and readable — drop the oldest once a collection
/// passes this many recorded runs.
pub const MAX_ENTRIES: usize = 100;

/// Same 1 MB read guard as the collection store — a corrupt/huge history file
/// can't OOM the process.
pub const MAX_BYTES: u64 = 1024 * 1024;

fn history_path(dir: &Path, collection_id: &str) -> AppResult<PathBuf> {
    Ok(dir.join(format!("{}.json", sanitize_name(collection_id)?)))
}

/// All recorded runs for a collection, oldest first. A missing file is an empty
/// history, not an error.
pub fn load(dir: &Path, collection_id: &str) -> AppResult<Vec<RunSummary>> {
    let path = history_path(dir, collection_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let len = std::fs::metadata(&path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!(
            "history file is too large ({len} bytes > {MAX_BYTES} cap)"
        )));
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

/// Append new run summaries to a collection's history, keeping only the most
/// recent `MAX_ENTRIES` so the log can't grow without bound.
pub fn append(dir: &Path, collection_id: &str, new: &[RunSummary]) -> AppResult<()> {
    let mut entries = load(dir, collection_id)?;
    entries.extend_from_slice(new);
    if entries.len() > MAX_ENTRIES {
        entries.drain(0..entries.len() - MAX_ENTRIES);
    }
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(&entries)?;
    std::fs::write(history_path(dir, collection_id)?, json)?;
    Ok(())
}

#[cfg(test)]
#[path = "eval_history_tests.rs"]
mod tests;

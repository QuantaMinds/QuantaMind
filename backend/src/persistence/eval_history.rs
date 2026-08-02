use crate::errors::{AppError, AppResult};
// `RunSummary` is a domain type owned by the eval engine; persistence is a driven
// adapter that depends on it (the edge points inward, never the reverse).
use crate::inference::eval::run_summary::RunSummary;
use crate::persistence::evals::sanitize_name;
use serde::Serialize;
use serde_json::Value;
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

/// A history read: the records this build can interpret, plus a count of the ones
/// it can't. A record written by a build with a different backend set (or any
/// other schema drift) is SKIPPED, never fatal — one legacy row must not take the
/// whole panel down — and never silently: the count is surfaced to the UI.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct LoadedHistory {
    pub entries: Vec<RunSummary>,
    pub unreadable: usize,
}

/// The file's raw records, untouched. Used by both `load` (which then interprets
/// each) and `append` (which must NOT drop what it couldn't interpret — rewriting
/// the file from parsed entries alone would silently delete the user's older runs).
fn load_raw(dir: &Path, collection_id: &str) -> AppResult<Vec<Value>> {
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

/// All recorded runs for a collection, oldest first. A missing file is an empty
/// history, not an error. Each record is deserialized INDEPENDENTLY so a single
/// undecodable row (e.g. one naming a backend this build no longer supports)
/// costs only that row.
pub fn load(dir: &Path, collection_id: &str) -> AppResult<LoadedHistory> {
    let raw = load_raw(dir, collection_id)?;
    let mut out = LoadedHistory::default();
    for v in raw {
        match serde_json::from_value::<RunSummary>(v) {
            Ok(e) => out.entries.push(e),
            Err(_) => out.unreadable += 1,
        }
    }
    Ok(out)
}

/// Append new run summaries to a collection's history, keeping only the most
/// recent `MAX_ENTRIES` so the log can't grow without bound. Operates on the RAW
/// records, so rows this build can't interpret survive the rewrite untouched.
pub fn append(dir: &Path, collection_id: &str, new: &[RunSummary]) -> AppResult<()> {
    let mut entries = load_raw(dir, collection_id)?;
    for e in new {
        entries.push(serde_json::to_value(e)?);
    }
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

use crate::errors::{AppError, AppResult};
use crate::inference::eval::batch::{BatchColumn, BatchReport};
use crate::persistence::readiness::safe_filename::safe_filename;
use std::path::{Path, PathBuf};

/// Same 1 MB read guard as the trace/history stores — a corrupt/huge report file
/// can't OOM the process. One collection's latest report stays well under this.
pub const MAX_BYTES: u64 = 1024 * 1024;

fn report_path(dir: &Path, collection_id: &str) -> PathBuf {
    dir.join(format!("{}.json", safe_filename(collection_id)))
}

/// Persist a collection's most-recent batch report (last-write-wins). Rust is the
/// source of truth for the readiness verdict — the report no longer lives only in
/// the frontend store, so the GUI command and a future CLI read the same bytes.
pub fn save(dir: &Path, report: &BatchReport) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(report_path(dir, &report.collection_id), json)?;
    Ok(())
}

/// The collection's last persisted report, or `None` when none has been saved yet
/// (a missing file is not an error — the readiness page shows an empty state).
pub fn load(dir: &Path, collection_id: &str) -> AppResult<Option<BatchReport>> {
    let path = report_path(dir, collection_id);
    if !path.exists() {
        return Ok(None);
    }
    let len = std::fs::metadata(&path)?.len();
    if len > MAX_BYTES {
        return Err(AppError::Validation(format!(
            "batch report file is too large ({len} bytes > {MAX_BYTES} cap)"
        )));
    }
    let content = std::fs::read_to_string(&path)?;
    // Deserialize the report shell, then each COLUMN independently. A column
    // recorded against a backend this build no longer supports would otherwise
    // fail the whole read, blanking the Agent Report page for the collection.
    // The dropped count rides along so the UI can name what it isn't showing —
    // a short verdict table must never read as the complete run.
    #[derive(serde::Deserialize)]
    struct RawReport {
        #[serde(flatten)]
        rest: serde_json::Value,
        #[serde(default)]
        columns: Vec<serde_json::Value>,
    }
    let raw: RawReport = serde_json::from_str(&content)?;
    let mut columns = Vec::with_capacity(raw.columns.len());
    let mut dropped = 0usize;
    for c in raw.columns {
        match serde_json::from_value::<BatchColumn>(c) {
            Ok(col) => columns.push(col),
            Err(_) => dropped += 1,
        }
    }
    // `rest` carries every field EXCEPT `columns` (serde(flatten) extracted it), and
    // `columns` is required on `BatchReport` — so put the interpreted ones back
    // before deserializing the shell.
    let mut rest = raw.rest;
    if let Some(obj) = rest.as_object_mut() {
        obj.insert("columns".into(), serde_json::to_value(&columns)?);
    }
    let mut report: BatchReport = serde_json::from_value(rest)?;
    report.unreadable_columns = dropped;
    Ok(Some(report))
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;

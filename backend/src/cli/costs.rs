//! `qm costs <collection>` — the LAST persisted run's per-task costs, read back from the
//! app's on-disk stores (`agentic_transcripts/` + `batch_reports/`), no model run needed.
//! Retention is latest-batch-only, so this always shows the most recent run of that
//! collection. Same honesty contract as `--costs` and the app's Latency view: "n/a" =
//! the backend reported nothing; labels are the SANITIZED transcript stems (the original
//! ids aren't recorded in the file — printed as-is, never "un-sanitized" by guesswork).

use crate::cli::run::costs::{self, RunCosts};
use crate::errors::{AppError, AppResult};
use crate::inference::eval::batch::{BatchReport, TaskOutcome};
use crate::persistence::jobs::transcripts::{load_records, TranscriptEntry};
use crate::persistence::readiness::safe_filename::safe_filename;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The desktop app's data dir (Tauri `app_config_dir` for `dev.quantamind.app`),
/// resolved WITHOUT Tauri so the CLI can read the same stores. `--data-dir` overrides.
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/dev.quantamind.app"))
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("dev.quantamind.app"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("dev.quantamind.app"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Split a transcript file stem — `<model-safe>--<task-safe>[--native]` — into its
/// parts. The safe stems contain no `--` themselves (`safe_filename` maps every
/// non-alphanumeric to a SINGLE `-` and trims), so the separator is unambiguous.
pub fn parse_stem(stem: &str) -> Option<(String, String, bool)> {
    let (rest, native) = match stem.strip_suffix("--native") {
        Some(r) => (r, true),
        None => (stem, false),
    };
    let (model, task) = rest.split_once("--")?;
    (!model.is_empty() && !task.is_empty()).then(|| (model.to_string(), task.to_string(), native))
}

/// Load the last run's costs for `collection`, one `RunCosts` per model found in its
/// transcripts. `report` (when the saved `batch_reports/<collection>.json` parses) maps
/// each model's stamped memory facts by SANITIZED name — the only join key the
/// transcript filenames preserve.
pub fn load_collection_costs(data_dir: &Path, collection: &str) -> AppResult<Vec<RunCosts>> {
    let dir = data_dir.join("agentic_transcripts").join(safe_filename(collection));
    if !dir.is_dir() {
        return Err(AppError::Validation(format!(
            "no persisted run for '{collection}' — run it in the app's Tests tab (or `qm run --costs` for a live run)"
        )));
    }
    let report: Option<BatchReport> = std::fs::read_to_string(data_dir.join("batch_reports").join(format!("{}.json", safe_filename(collection))))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let column_by_safe: BTreeMap<String, _> = report
        .as_ref()
        .map(|r| r.columns.iter().map(|c| (safe_filename(&c.model), c)).collect())
        .unwrap_or_default();

    // model-safe → (task-safe, native) → steps/outcome.
    let mut cells: BTreeMap<String, BTreeMap<(String, bool), Vec<_>>> = BTreeMap::new();
    let mut outcomes: BTreeMap<String, BTreeMap<(String, bool), TaskOutcome>> = BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| AppError::Io(e.to_string()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    entries.sort(); // stable output order
    for path in entries {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Some((model, task, native)) = parse_stem(stem) else { continue };
        for rec in load_records(&path)? {
            match rec {
                TranscriptEntry::Step(s) => cells
                    .entry(model.clone())
                    .or_default()
                    .entry((task.clone(), native))
                    .or_default()
                    .push(s),
                TranscriptEntry::Outcome(o) => {
                    outcomes.entry(model.clone()).or_default().insert((task.clone(), native), o);
                }
            }
        }
    }
    if cells.is_empty() {
        return Err(AppError::Validation(format!("'{collection}' has transcripts but no readable steps")));
    }

    Ok(cells
        .into_iter()
        .map(|(model_safe, model_cells)| {
            let empty = BTreeMap::new();
            let outs = outcomes.get(&model_safe).unwrap_or(&empty);
            // Offline: no live server to dim-probe — the KV-at-peak figure stays None
            // (the app shows it live; here we only report what the disk recorded).
            costs::assemble(&model_safe, &model_cells, outs, column_by_safe.get(&model_safe).copied(), None)
        })
        .collect())
}

#[cfg(test)]
#[path = "costs_cli_tests.rs"]
mod tests;

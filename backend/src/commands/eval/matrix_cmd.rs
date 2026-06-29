use crate::commands::eval::toolcall_cmd::{endpoint_for, traces_dir};
use crate::errors::AppError;
use crate::inference::eval::toolcall::eval::run_eval_traced;
use crate::inference::eval::toolcall::matrix::{build_matrix, summaries, MatrixReport, ModelTarget};
use crate::inference::eval::run_summary::RunSummary;
use crate::inference::eval::toolcall::tasks::{validate_tasks, ToolTask};
use crate::persistence::eval_history;
use crate::persistence::eval_trace_store;
use std::path::PathBuf;
use tauri::Manager;

/// Managed dir for per-collection regression logs (mirrors the `evals/` dir).
fn history_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("history"))
}

/// Run a collection against several models **sequentially** (local backends don't
/// like concurrent load) and return a matrix of per-model reports. One model's
/// failure is captured as that column's error — it never aborts the batch. The
/// successful columns are appended to the collection's regression history.
#[tauri::command]
pub async fn run_collection_matrix(
    app: tauri::AppHandle,
    collection_id: String,
    targets: Vec<ModelTarget>,
    tasks: Vec<ToolTask>,
) -> Result<MatrixReport, AppError> {
    validate_tasks(&tasks)?;
    let trace_dir = traces_dir(&app).ok();
    let mut results = Vec::with_capacity(targets.len());
    for target in targets {
        // Keep the report for the matrix; cache the full traces (best-effort) so a
        // cell drill-down shows them without re-running. A down model is captured
        // as that column's error and never aborts the batch.
        let res = run_eval_traced(target.backend, &endpoint_for(target.backend), &target.model, &tasks, None)
            .await
            .map(|(report, traces)| {
                if let Some(dir) = &trace_dir {
                    let _ = eval_trace_store::upsert(dir, &collection_id, &target.model, target.backend, &traces);
                }
                report
            });
        results.push((target, res));
    }
    let report = build_matrix(&collection_id, results);
    let entries = summaries(&report, &crate::time_iso::now_utc());
    if !entries.is_empty() {
        eval_history::append(&history_dir(&app)?, &collection_id, &entries)?;
    }
    Ok(report)
}

/// The recorded run history for a collection, oldest first (for the timeline).
#[tauri::command]
pub fn load_collection_history(
    app: tauri::AppHandle,
    collection_id: String,
) -> Result<Vec<RunSummary>, AppError> {
    eval_history::load(&history_dir(&app)?, &collection_id)
}

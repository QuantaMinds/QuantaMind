use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::v2::oracle::{validate_collection_deep, CollectionValidation};
use crate::inference::eval::toolcall::tasks::ToolTask;
use crate::persistence::evals;
use std::path::PathBuf;
use tauri::Manager;

/// The managed directory for user-authored eval collections: one `.json` per
/// collection under the app config dir (mirrors the workspaces recents path).
fn evals_dir(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("evals"))
}

#[tauri::command]
pub fn list_custom_collections(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    evals::list(&evals_dir(&app)?)
}

#[tauri::command]
pub fn load_custom_collection(app: tauri::AppHandle, name: String) -> Result<Vec<ToolTask>, AppError> {
    evals::load(&evals_dir(&app)?, &name)
}

#[tauri::command]
pub fn save_custom_collection(
    app: tauri::AppHandle,
    name: String,
    tasks: Vec<ToolTask>,
) -> Result<(), AppError> {
    evals::save(&evals_dir(&app)?, &name, &tasks)
}

#[tauri::command]
pub fn delete_custom_collection(app: tauri::AppHandle, name: String) -> Result<(), AppError> {
    evals::delete(&evals_dir(&app)?, &name)
}

/// Deep-validate a saved custom collection: structural schema check + the oracle "answer key
/// works" proof (each task reachable by a perfect agent, and a do-nothing agent fails it). Runs
/// entirely offline — no model, no server — so the author can confirm their tasks are solvable
/// and discriminating in seconds before spending a real run. A `no`/non-discriminating task
/// means the ANSWER KEY is broken, not that models are bad at it.
#[tauri::command]
pub async fn validate_custom_collection(
    app: tauri::AppHandle,
    name: String,
) -> Result<CollectionValidation, AppError> {
    let tasks = evals::load(&evals_dir(&app)?, &name)?;
    let mut v = validate_collection_deep(&tasks).await;
    // World tasks (agentic.mcp): fold in the static + live do-nothing checks — the
    // findings land in the same fields this UI already renders.
    crate::inference::eval::mcp::validate::merge_world_checks(&mut v, &tasks, true).await;
    Ok(v)
}

/// Dry-run the same deep validation on an external `.json` file BEFORE importing it — so a
/// broken collection is caught (and its bad tasks named) without being written into the managed
/// dir. The frontend never reads file contents; it passes the picked path.
#[tauri::command]
pub async fn validate_collection_file(source_path: PathBuf) -> Result<CollectionValidation, AppError> {
    let tasks = evals::read_capped(&source_path)?;
    let mut v = validate_collection_deep(&tasks).await;
    crate::inference::eval::mcp::validate::merge_world_checks(&mut v, &tasks, true).await;
    Ok(v)
}

/// Read a picked text file (e.g. a CSV) by PATH with the size cap, returning its
/// raw contents. The frontend never reads files itself; for CSV import it then
/// parses + validates the text and saves via `save_custom_collection`.
#[tauri::command]
pub fn read_text_capped(source_path: PathBuf) -> Result<String, AppError> {
    evals::read_text_capped(&source_path)
}

/// Import an external `.json` collection by PATH (the frontend never reads file
/// contents): read it with the size cap, validate, derive a safe name from the
/// file stem, write it into the managed dir, and return the new name.
#[tauri::command]
pub fn import_custom_collection(
    app: tauri::AppHandle,
    source_path: PathBuf,
) -> Result<String, AppError> {
    let tasks = evals::read_capped(&source_path)?;
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::Validation("import file has no usable name".into()))?;
    let name = evals::sanitize_name(stem)?;
    let dir = evals_dir(&app)?;
    evals::save(&dir, &name, &tasks)?;
    Ok(name)
}

use crate::errors::{AppError, AppResult};
use crate::persistence::fs_guard;
use crate::persistence::prompts::{tree, tree::TreeNode};
use crate::persistence::workspaces::{
    load as load_recents, record as record_recent, save as save_recents, RecentEntry, RecentList,
};
use crate::sync::MutexExt;
use crate::time_iso::now_utc;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

pub const RECENTS_FILE: &str = "recent_workspaces.yaml";

#[derive(Default)]
pub struct WorkspaceState {
    root: Mutex<Option<PathBuf>>,
}

impl WorkspaceState {
    pub fn root(&self) -> AppResult<PathBuf> {
        self.root
            .lock_recover()
            .clone()
            .ok_or_else(|| AppError::Validation("no workspace open".into()))
    }
    pub fn ensure_within(&self, candidate: &Path) -> AppResult<PathBuf> {
        fs_guard::ensure_within(&self.root()?, candidate)
    }
    /// Resolve a path for creating a new file inside the workspace. Same confinement as
    /// `ensure_within` (the shared `fs_guard`): the parent is canonicalised + confined, and a
    /// symlink final component that escapes root is rejected. The file itself need not exist.
    pub fn resolve_new(&self, candidate: &Path) -> AppResult<PathBuf> {
        fs_guard::ensure_within(&self.root()?, candidate)
    }
    fn set(&self, p: PathBuf) {
        *self.root.lock_recover() = Some(p);
    }
}

fn recents_path(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join(RECENTS_FILE))
}

#[tauri::command]
pub fn open_workspace(
    app: tauri::AppHandle,
    state: tauri::State<'_, WorkspaceState>,
    path: String,
) -> Result<Vec<TreeNode>, AppError> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(AppError::Validation(format!("not a directory: {path}")));
    }
    let abs = root.canonicalize().map_err(|e| AppError::Io(e.to_string()))?;
    state.set(abs.clone());
    let mut list = load_recents(&recents_path(&app)?)?;
    record_recent(&mut list, RecentEntry { path: abs.display().to_string(), opened_at: now_utc() });
    save_recents(&recents_path(&app)?, &list)?;
    tree::list(&abs)
}

#[tauri::command]
pub fn close_workspace(state: tauri::State<'_, WorkspaceState>) -> Result<(), AppError> {
    *state.root.lock_recover() = None;
    Ok(())
}

#[tauri::command]
pub fn current_workspace(state: tauri::State<'_, WorkspaceState>) -> Result<Option<String>, AppError> {
    Ok(state.root.lock_recover().as_ref().map(|p| p.display().to_string()))
}

#[tauri::command]
pub fn list_workspace_tree(
    state: tauri::State<'_, WorkspaceState>,
) -> Result<Vec<TreeNode>, AppError> {
    tree::list(&state.root()?)
}

#[tauri::command]
pub fn recent_workspaces(app: tauri::AppHandle) -> Result<RecentList, AppError> {
    load_recents(&recents_path(&app)?)
}

#[cfg(test)]
#[path = "workspaces_tests.rs"]
mod tests;

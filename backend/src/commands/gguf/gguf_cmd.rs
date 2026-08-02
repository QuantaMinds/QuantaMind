use crate::commands::emit::log_emit;
use crate::commands::settings::user_settings::UserSettingsState;
use crate::commands::storage::storage_disk::gguf_dest;
use crate::errors::{AppError, AppResult};
use crate::inference::gguf::gguf::{inspect_gguf as inspect, GgufMetadata};
use crate::inference::pull::pull_name::validate_name;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

pub const EVENT_MODELS_CHANGED: &str = "models-changed";

/// Where to copy a local-file install inside the shared folder, or `None` when
/// the picked file is already that exact path (no copy needed).
pub fn retain_dest(dir: &Path, name: &str, src: &Path) -> Option<PathBuf> {
    let dest = gguf_dest(dir, name);
    (src != dest).then_some(dest)
}

#[tauri::command]
pub async fn inspect_gguf(path: String) -> Result<GgufMetadata, AppError> {
    inspect(&PathBuf::from(&path))
}

/// Validate a picked file is a real, readable GGUF before we copy multiple GB of
/// it. Returns its parsed metadata so the caller need not re-read the header.
pub fn validate_gguf_source(path: &str, name: &str) -> AppResult<GgufMetadata> {
    validate_name(name)?;
    let p = PathBuf::from(path);
    if !p.exists() {
        return Err(AppError::Validation(format!("file does not exist: {path}")));
    }
    let ext_ok = p.extension().and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("gguf")).unwrap_or(false);
    if !ext_ok {
        return Err(AppError::Validation(format!("not a .gguf file: {path}")));
    }
    inspect(&p)
}

/// Install a local `.gguf` by placing it in the shared weights folder, where
/// `llama-server` loads it from. A copy only happens when the picked file isn't
/// already at its destination path.
#[tauri::command]
pub async fn install_local_gguf(
    app: AppHandle,
    settings: tauri::State<'_, UserSettingsState>,
    path: String,
    name: String,
) -> AppResult<()> {
    let dir = settings.weights_dir(&app)?;
    validate_gguf_source(&path, &name)?;
    if let Some(dest) = retain_dest(&dir, &name, Path::new(&path)) {
        fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
        fs::copy(&path, &dest).map_err(|e| AppError::Io(e.to_string()))?;
    }
    log_emit(&app, EVENT_MODELS_CHANGED, ());
    Ok(())
}

#[cfg(test)]
#[path = "gguf_cmd_tests.rs"]
mod tests;

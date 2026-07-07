use crate::commands::storage::storage_disk::absolutize;
use crate::errors::{AppError, AppResult};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Regenerable caches under `app_config_dir`, safe to wipe to reclaim space.
/// Each is rebuilt on the next run. User-authored data (`evals/`, `readiness/`)
/// and settings (`user_settings.yaml`, `model_settings.yaml`) are deliberately
/// absent from this allow-list so a cache clear never destroys them.
const CACHE_DIRS: &[&str] = &["jobs", "history", "batch_reports", "traces", "cliff", "agentic_transcripts"];

/// Regenerable cache files (the recent-workspace list is navigation history).
const CACHE_FILES: &[&str] = &["recent_workspaces.yaml"];

/// HuggingFace cache subdirs holding regenerable model/dataset snapshots. The
/// MLX/STT tooling caches downloads here, separate from the app's canonical
/// weights in `~/.quantamind`. The auth `token`/`stored_tokens` files sit
/// alongside these and are deliberately excluded so a clear never signs the
/// user out — only the re-downloadable snapshots are removed.
const HF_CACHE_SUBDIRS: &[&str] = &["hub", "xet", "assets", "datasets"];

/// Sum the byte size of every file under `dir` (recursively). Missing dir → 0.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Delete every regenerable cache under `base`, returning the total bytes freed
/// (measured from on-disk file sizes before removal). A target that doesn't
/// exist is skipped cleanly — clearing an already-empty cache is not an error.
pub fn clear_cache_in(base: &Path) -> AppResult<u64> {
    let mut freed = 0u64;
    for name in CACHE_DIRS {
        let path = base.join(name);
        if path.is_dir() {
            freed += dir_size(&path);
            std::fs::remove_dir_all(&path).map_err(|e| AppError::Io(e.to_string()))?;
        }
    }
    for name in CACHE_FILES {
        let path = base.join(name);
        if path.is_file() {
            freed += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            std::fs::remove_file(&path).map_err(|e| AppError::Io(e.to_string()))?;
        }
    }
    Ok(freed)
}

/// Resolve the HuggingFace cache root. The HF library (MLX/STT model loads)
/// caches snapshots here, separate from the app's own `~/.quantamind` weights.
/// Honors `HF_HOME`, else falls back to `$HOME/.cache/huggingface`.
fn hf_cache_dir() -> PathBuf {
    if let Ok(p) = std::env::var("HF_HOME") {
        if !p.trim().is_empty() {
            return absolutize(PathBuf::from(p));
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home).join(".cache/huggingface")
}

/// Delete the regenerable HuggingFace snapshot caches under `hf_home`, returning
/// total bytes freed. Auth tokens are preserved; a missing subdir is skipped
/// cleanly. The snapshots re-download on next use, so this is reclaimable space.
pub fn clear_hf_cache_in(hf_home: &Path) -> AppResult<u64> {
    let mut freed = 0u64;
    for name in HF_CACHE_SUBDIRS {
        let path = hf_home.join(name);
        if path.is_dir() {
            freed += dir_size(&path);
            std::fs::remove_dir_all(&path).map_err(|e| AppError::Io(e.to_string()))?;
        }
    }
    Ok(freed)
}

/// Clear regenerable app caches (eval history, batch reports, job logs, traces,
/// context-cliff measurements, recent-workspace list). Returns bytes freed.
/// When `include_models` is set, also wipes the HuggingFace snapshot cache
/// (re-downloadable MLX/whisper weights) — the app's canonical `~/.quantamind`
/// models, custom eval collections, readiness profiles, and user settings are
/// never touched.
#[tauri::command]
pub fn clear_app_cache(app: AppHandle, include_models: bool) -> Result<u64, AppError> {
    let base = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    let mut freed = clear_cache_in(&base)?;
    if include_models {
        freed += clear_hf_cache_in(&hf_cache_dir())?;
    }
    Ok(freed)
}

#[cfg(test)]
#[path = "storage_cache_tests.rs"]
mod tests;

use crate::commands::storage::storage_disk::compute_disk_usage;
use crate::errors::AppError;
use serde::Serialize;
use std::path::{Path, PathBuf};

// Need >=50GB free at the new location to seriously consider it for
// model storage — typical mid-size models are 4-8GB each and users
// install several.
const MIN_FREE_BYTES: u64 = 50 * 1024 * 1024 * 1024;


#[derive(Serialize, Clone)]
pub struct PathValidation {
    pub exists: bool,
    pub is_dir: bool,
    pub writable: bool,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub sufficient: bool,
}

/// Test that the directory supports both write AND rename, since the
/// HF download resume flow needs to rename `<file>.partial` → `<file>`
/// at the end of every download. Probes both operations; any failure
/// fails the check.
fn test_writable(p: &Path) -> bool {
    let probe = p.join(".quantamind-write-probe");
    let renamed = p.join(".quantamind-rename-probe");
    let write_ok = std::fs::write(&probe, b"").is_ok();
    let rename_ok = write_ok && std::fs::rename(&probe, &renamed).is_ok();
    let _ = std::fs::remove_file(&probe);
    let _ = std::fs::remove_file(&renamed);
    write_ok && rename_ok
}

#[tauri::command]
pub fn validate_storage_path(path: String) -> Result<PathValidation, AppError> {
    let p = PathBuf::from(&path);
    let exists = p.exists();
    let is_dir = exists && p.is_dir();
    let writable = is_dir && test_writable(&p);
    let usage = compute_disk_usage(&p, 0);
    Ok(PathValidation {
        exists,
        is_dir,
        writable,
        free_bytes: usage.free_bytes,
        total_bytes: usage.total_bytes,
        sufficient: usage.free_bytes >= MIN_FREE_BYTES,
    })
}

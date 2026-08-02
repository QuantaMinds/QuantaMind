use crate::commands::storage::storage_disk::{compute_disk_usage, gguf_dir};
use crate::commands::storage::storage_types::DiskUsage;
use crate::errors::AppError;
use std::path::Path;

/// Total bytes of the `.gguf` weights in `dir`. Non-recursive (the shared folder
/// is flat) and best-effort: an unreadable dir or entry contributes 0 rather
/// than failing the whole panel, since free/total disk info is still useful.
pub fn weights_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.path().extension().and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
        })
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Disk usage for the shared weights folder. Storage info (free/total) never
/// depends on a model runtime, so this reads the files directly rather than
/// asking a server what it has installed.
pub fn disk_usage_for(weights_dir: &Path) -> DiskUsage {
    compute_disk_usage(weights_dir, weights_bytes(weights_dir))
}

#[tauri::command]
pub async fn get_disk_usage() -> Result<DiskUsage, AppError> {
    Ok(disk_usage_for(&gguf_dir()))
}

#[cfg(test)]
#[path = "storage_usage_tests.rs"]
mod tests;

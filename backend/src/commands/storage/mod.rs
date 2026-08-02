// `storage_disk` (+ the plain `DiskUsage` type it returns) is pure path/disk
// logic with no Tauri dependency, so it stays in the headless `qm` build — the
// CLI resolves the shared weights folder through it. The command wrappers that
// need an `AppHandle` are GUI-only.
pub mod storage_disk;
pub mod storage_types;
#[cfg(feature = "gui")]
pub mod storage_cache;
#[cfg(feature = "gui")]
pub mod storage_usage;

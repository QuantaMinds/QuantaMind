use crate::commands::storage::storage_types::DiskUsage;
use crate::os;
use std::path::{Path, PathBuf};
use sysinfo::Disks;

/// Make a path absolute so the UI never shows a relative/"hidden" path like
/// `./`. Relative paths are joined onto the current working directory.
pub(crate) fn absolutize(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p)
}

/// Shared GGUF weights folder — the source of truth for installed models. HF and
/// local-file downloads are retained here and `llama-server` loads them directly.
/// Precedence: user setting → `QUANTAMIND_GGUF_DIR` env →
/// `~/.quantamind/gguf` on Unix / `%LOCALAPPDATA%\QuantaMind\gguf` on Windows
/// (via `os::user_dirs::data_dir()`).
pub fn gguf_dir_resolved(setting: Option<&str>) -> PathBuf {
    if let Some(p) = setting.filter(|s| !s.trim().is_empty()) {
        return absolutize(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("QUANTAMIND_GGUF_DIR") {
        return absolutize(PathBuf::from(p));
    }
    os::user_dirs::data_dir().join("gguf")
}

/// The default/env-resolved weights folder (no user-setting override).
pub fn gguf_dir() -> PathBuf {
    gguf_dir_resolved(None)
}

/// Path for a GGUF named `name`, sanitizing `:`/`/` so a model tag like
/// `llama3.2:1b` maps to a safe `llama3.2_1b.gguf` filename.
pub fn gguf_dest(dir: &Path, name: &str) -> PathBuf {
    let safe = name.replace([':', '/'], "_");
    dir.join(format!("{safe}.gguf"))
}

/// Resolve a llama.cpp column's GGUF on disk under `dir`: the sanitized `gguf_dest`
/// mapping for a model tag, or the name joined verbatim when it already ends in `.gguf`.
/// `None` when the file is absent (the caller then treats data as unmeasured, never guessed).
pub fn find_gguf(dir: &Path, model: &str) -> Option<PathBuf> {
    let path = if model.ends_with(".gguf") { dir.join(model) } else { gguf_dest(dir, model) };
    path.exists().then_some(path)
}

/// `find_gguf` against the default/env-resolved weights folder — the common case for
/// commands that don't carry the user-setting override.
pub fn find_installed_gguf(model: &str) -> Option<PathBuf> {
    find_gguf(&gguf_dir(), model)
}

/// On Windows only, warn via stderr if the legacy `~/.quantamind/gguf` folder
/// exists alongside the new `%LOCALAPPDATA%\QuantaMind`
/// default. **Never auto-move** — user model weights are irreplaceable, and a
/// bad move here is unforgivable. The user must migrate manually. Idempotent —
/// runs once at startup and just logs; the app keeps using the new default so
/// nothing breaks.
pub fn warn_on_legacy_windows_paths() {
    #[cfg(windows)]
    {
        if std::env::var("QUANTAMIND_GGUF_DIR").is_ok() || std::env::var("QUANTAMIND_the remote server_DIR").is_ok() {
            return; // user has an explicit override — no legacy to warn about
        }
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return,
        };
        let legacy_gguf = home.join(".quantamind").join("gguf");
        if legacy_gguf.exists() {
            let new_dir = os::user_dirs::data_dir();
            eprintln!(
                "[storage] legacy weights folder detected at {}\\gguf — QuantaMind now defaults to {}. To migrate, move the files manually (nothing is auto-moved). Set QUANTAMIND_GGUF_DIR to point at the legacy location if you prefer to keep it.",
                home.join(".quantamind").display(),
                new_dir.display(),
            );
        }
    }
}

/// Compute total/free bytes for the disk that holds `probe_path`, plus
/// the caller-supplied sum of all model blob sizes (from /api/tags).
/// Falls back to zero if no disk matches (e.g. exotic mount layout).
pub fn compute_disk_usage(probe_path: &Path, models_bytes: u64) -> DiskUsage {
    let disks = Disks::new_with_refreshed_list();
    let best = disks
        .list()
        .iter()
        .filter(|d| probe_path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    let (total, free) = match best {
        Some(d) => (d.total_space(), d.available_space()),
        None => (0u64, 0u64),
    };
    DiskUsage {
        total_bytes: total,
        free_bytes: free,
        models_bytes,
    }
}

#[cfg(test)]
#[path = "storage_disk_tests.rs"]
mod tests;

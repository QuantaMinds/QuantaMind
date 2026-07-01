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

/// Resolve the on-disk Ollama models directory. Respects `OLLAMA_MODELS`
/// if set; otherwise defaults to `$HOME/.ollama/models` (via `dirs::home_dir`,
/// which reads `USERPROFILE` on Windows and `HOME` on Unix — so Windows works
/// without any env-var setup, matching the Ollama installer's default).
pub fn models_dir() -> PathBuf {
    if let Ok(p) = std::env::var("OLLAMA_MODELS") {
        return absolutize(PathBuf::from(p));
    }
    dirs::home_dir()
        .map(|h| h.join(".ollama").join("models"))
        .unwrap_or_else(|| PathBuf::from(".ollama/models"))
}

/// Shared GGUF weights folder, the source of truth for both backends. HF and
/// local-file downloads are retained here (llama.cpp loads them directly;
/// Ollama imports them). Precedence: user setting → `QUANTAMIND_GGUF_DIR` env →
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

/// Local MLX weights folder. Each MLX repo is snapshotted into its own subdir
/// here (multi-file safetensors models, unlike single-file GGUF). Precedence:
/// user setting → `QUANTAMIND_MLX_DIR` env → `~/.quantamind/mlx` on Unix /
/// `%LOCALAPPDATA%\QuantaMind\mlx` on Windows.
pub fn mlx_dir_resolved(setting: Option<&str>) -> PathBuf {
    if let Some(p) = setting.filter(|s| !s.trim().is_empty()) {
        return absolutize(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("QUANTAMIND_MLX_DIR") {
        return absolutize(PathBuf::from(p));
    }
    os::user_dirs::data_dir().join("mlx")
}

/// The default/env-resolved MLX weights folder (no user-setting override).
pub fn mlx_dir() -> PathBuf {
    mlx_dir_resolved(None)
}

/// Subdirectory holding one MLX repo's snapshot, sanitizing `/`/`:` so
/// `mlx-community/Llama-3.2-3B-Instruct-4bit` maps to a safe
/// `mlx-community_Llama-3.2-3B-Instruct-4bit` folder.
pub fn mlx_model_dir(dir: &Path, repo: &str) -> PathBuf {
    let safe = repo.replace(['/', ':'], "_");
    dir.join(safe)
}

/// On Windows only, warn via stderr if the legacy `~/.quantamind/gguf` or
/// `~/.quantamind/mlx` folders exist alongside the new `%LOCALAPPDATA%\QuantaMind`
/// default. **Never auto-move** — user model weights are irreplaceable, and a
/// bad move here is unforgivable. The user must migrate manually. Idempotent —
/// runs once at startup and just logs; the app keeps using the new default so
/// nothing breaks.
pub fn warn_on_legacy_windows_paths() {
    #[cfg(windows)]
    {
        if std::env::var("QUANTAMIND_GGUF_DIR").is_ok() || std::env::var("QUANTAMIND_MLX_DIR").is_ok() {
            return; // user has an explicit override — no legacy to warn about
        }
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return,
        };
        let legacy_gguf = home.join(".quantamind").join("gguf");
        let legacy_mlx = home.join(".quantamind").join("mlx");
        if legacy_gguf.exists() || legacy_mlx.exists() {
            let new_dir = os::user_dirs::data_dir();
            eprintln!(
                "[storage] legacy weights folder detected at {}\\{{gguf,mlx}} — QuantaMind now defaults to {}. To migrate, move the files manually (nothing is auto-moved). Set QUANTAMIND_GGUF_DIR / QUANTAMIND_MLX_DIR to point at the legacy location if you prefer to keep it.",
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
        ollama_models_bytes: models_bytes,
    }
}

#[cfg(test)]
#[path = "storage_disk_tests.rs"]
mod tests;

use std::path::{Path, PathBuf};

const EXE: &str = "mlx_lm.server";

/// Directories to search for `mlx_lm.server`, in priority order: every `PATH`
/// entry, then common venv/conda bins under `home`, then Homebrew. Pure given
/// `home` + `path_env`, so it's testable without touching the environment.
pub fn candidate_dirs(home: Option<&str>, path_env: Option<&str>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(p) = path_env {
        dirs.extend(std::env::split_paths(p));
    }
    if let Some(h) = home {
        for sub in ["mlx-env/bin", ".venv/bin", "miniconda3/bin"] {
            dirs.push(Path::new(h).join(sub));
        }
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

/// First candidate dir that contains `mlx_lm.server`. `exists` is injected so
/// the search is unit-testable without a real filesystem.
pub fn resolve_in(dirs: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(EXE)).find(|p| exists(p))
}

/// Resolve `mlx_lm.server`: an explicit `configured` full path wins (if it
/// exists), else search PATH + common venv locations. `None` → not installed /
/// path not set.
pub fn locate(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = configured.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    let home = std::env::var("HOME").ok();
    let path_env = std::env::var("PATH").ok();
    resolve_in(&candidate_dirs(home.as_deref(), path_env.as_deref()), |p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#[cfg(unix)]`-gated: the assertion checks for `/usr/bin` and
    /// `/opt/homebrew/bin` which don't exist on Windows. MLX itself is
    /// Apple-Silicon-only (`mlx_supported()` gates all runtime paths on
    /// `cfg!(all(target_os = "macos", target_arch = "aarch64"))`), so the
    /// locator this test covers never runs on Windows regardless — the test
    /// is meaningful only where the code it tests actually executes.
    #[cfg(unix)]
    #[test]
    fn candidate_dirs_covers_path_entries_venvs_and_homebrew() {
        let dirs = candidate_dirs(Some("/Users/x"), Some("/usr/bin:/bin"));
        assert!(dirs.contains(&PathBuf::from("/usr/bin")));
        assert!(dirs.iter().any(|d| d.ends_with("mlx-env/bin")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn resolve_in_picks_the_first_dir_that_has_the_exe() {
        let dirs = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let found = resolve_in(&dirs, |p| p == Path::new("/b/mlx_lm.server"));
        assert_eq!(found, Some(PathBuf::from("/b/mlx_lm.server")));
        assert!(resolve_in(&dirs, |_| false).is_none());
    }
}

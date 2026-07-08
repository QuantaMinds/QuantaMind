use std::path::{Path, PathBuf};

const EXE: &str = "mlx_lm.server";

/// Known-safe install locations for `mlx_lm.server` (venv/conda bins under `home`, then
/// Homebrew). Preferred over raw `$PATH` so a PATH-poisoned binary earlier in `$PATH` cannot
/// win silently. Pure given `home`, so it's testable without touching the environment.
pub fn known_dirs(home: Option<&str>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(h) = home {
        for sub in ["mlx-env/bin", ".venv/bin", "miniconda3/bin"] {
            dirs.push(Path::new(h).join(sub));
        }
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

/// The raw `$PATH` entries — the LAST-RESORT search, only after `known_dirs` misses.
pub fn path_dirs(path_env: Option<&str>) -> Vec<PathBuf> {
    path_env.map(|p| std::env::split_paths(p).collect()).unwrap_or_default()
}

/// All candidate dirs in resolution priority: known-safe locations first, then `$PATH`.
pub fn candidate_dirs(home: Option<&str>, path_env: Option<&str>) -> Vec<PathBuf> {
    let mut dirs = known_dirs(home);
    dirs.extend(path_dirs(path_env));
    dirs
}

/// First candidate dir that contains `mlx_lm.server`. `exists` is injected so
/// the search is unit-testable without a real filesystem.
pub fn resolve_in(dirs: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(EXE)).find(|p| exists(p))
}

/// Resolve `mlx_lm.server`: an explicit `configured` full path wins (if it exists), then the
/// known-safe install locations, and only as a LAST RESORT the raw `$PATH` — logging a redacted
/// warning in that case so a PATH-based (poisoning-prone) resolution is never silent. `None` →
/// not installed / path not set.
pub fn locate(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = configured.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    let home = std::env::var("HOME").ok();
    if let Some(p) = resolve_in(&known_dirs(home.as_deref()), |p| p.exists()) {
        return Some(p);
    }
    let path_env = std::env::var("PATH").ok();
    let found = resolve_in(&path_dirs(path_env.as_deref()), |p| p.exists());
    if let Some(p) = &found {
        eprintln!(
            "[mlx] resolved mlx_lm.server from $PATH ({}) — set an explicit engine path in \
             Settings to avoid PATH-based resolution.",
            crate::redact::redact_path(&p.display().to_string())
        );
    }
    found
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

    /// Security ordering: known-safe dirs (homebrew/venv) come BEFORE raw $PATH, so a
    /// PATH-poisoned mlx_lm.server earlier in $PATH cannot win.
    #[cfg(unix)]
    #[test]
    fn known_dirs_precede_path_entries() {
        let all = candidate_dirs(Some("/Users/x"), Some("/tmp/evil:/usr/bin"));
        let homebrew = all.iter().position(|d| d == &PathBuf::from("/opt/homebrew/bin")).unwrap();
        let evil = all.iter().position(|d| d == &PathBuf::from("/tmp/evil")).unwrap();
        assert!(homebrew < evil, "known-safe dirs must be searched before $PATH: {all:?}");
    }
}

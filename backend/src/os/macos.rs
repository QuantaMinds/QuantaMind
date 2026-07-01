// macOS-only. This file is not even parsed on Windows/Linux builds — the
// `pub mod macos;` in `os/mod.rs` is `#[cfg(target_os = "macos")]`-gated.

use crate::os::EngineHost;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct MacosHost;

impl EngineHost for MacosHost {
    fn resolve_on_path(bin: &str) -> Option<PathBuf> {
        let out = Command::new("which").arg(bin).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if path.is_empty() {
            return None;
        }
        PathBuf::from(&path).parent().map(|p| p.to_path_buf())
    }

    fn envs_for_lib_dir(dir: &Path) -> Vec<(&'static str, PathBuf)> {
        // macOS: dyld's fallback library path resolves `@rpath` / `@loader_path`
        // when the sidecar's dylibs sit alongside the .exe.
        vec![("DYLD_FALLBACK_LIBRARY_PATH", dir.to_path_buf())]
    }

    fn apply_spawn_flags(_cmd: &mut Command) {
        // Unix needs no extra spawn flags — env vars from `envs_for_lib_dir`
        // are applied at call sites.
    }

    fn graceful_stop(pid: u32) -> Result<(), String> {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn hard_stop(pid: u32) -> Result<(), String> {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn pid_alive(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envs_for_lib_dir_uses_dyld_var() {
        let envs = MacosHost::envs_for_lib_dir(Path::new("/lib"));
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].0, "DYLD_FALLBACK_LIBRARY_PATH");
    }

    #[test]
    fn pid_alive_matches_self_and_dead_child() {
        assert!(MacosHost::pid_alive(std::process::id()));
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!MacosHost::pid_alive(pid));
    }

    #[test]
    fn hard_stop_on_dead_pid_is_ok() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(MacosHost::hard_stop(pid).is_ok());
    }

    #[test]
    fn resolve_on_path_finds_sh() {
        let dir = MacosHost::resolve_on_path("sh").expect("sh must be on PATH");
        assert!(dir.exists(), "resolved dir {dir:?} should exist");
    }
}

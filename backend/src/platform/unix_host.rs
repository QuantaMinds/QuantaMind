use crate::platform::EngineHost;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct UnixHost;

impl EngineHost for UnixHost {
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
        #[cfg(target_os = "macos")]
        {
            vec![("DYLD_FALLBACK_LIBRARY_PATH", dir.to_path_buf())]
        }
        #[cfg(target_os = "linux")]
        {
            vec![("LD_LIBRARY_PATH", dir.to_path_buf())]
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = dir;
            Vec::new()
        }
    }

    fn apply_spawn_flags(_cmd: &mut Command) {
        // Unix needs no extra spawn flags — env vars from `envs_for_lib_dir`
        // are applied at call sites.
    }

    fn graceful_stop(pid: u32) -> Result<(), String> {
        // Shell out to `kill` — the app already uses this idiom elsewhere and
        // one extra process spawn is cheap next to the grace window that
        // follows. `kill -TERM` on a gone pid exits 1 (already dead), which we
        // treat as success (caller wanted it stopped).
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
        // `kill -0` sends no signal; succeeds only when the pid exists and is
        // signalable — the standard Unix liveness probe.
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
    fn envs_for_lib_dir_names_the_right_var() {
        let envs = UnixHost::envs_for_lib_dir(Path::new("/lib"));
        assert_eq!(envs.len(), 1);
        let (k, _) = envs[0];
        #[cfg(target_os = "macos")]
        assert_eq!(k, "DYLD_FALLBACK_LIBRARY_PATH");
        #[cfg(target_os = "linux")]
        assert_eq!(k, "LD_LIBRARY_PATH");
    }

    #[test]
    fn pid_alive_matches_self_and_dead_child() {
        assert!(UnixHost::pid_alive(std::process::id()));
        // Spawn a trivial child, reap it, then confirm its pid is dead.
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!UnixHost::pid_alive(pid));
    }

    #[test]
    fn hard_stop_on_dead_pid_is_ok() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(UnixHost::hard_stop(pid).is_ok());
    }

    #[test]
    fn resolve_on_path_finds_a_standard_binary() {
        // `sh` exists on macOS + Linux; `which sh` should succeed and its
        // parent (typically `/bin`) is a real directory.
        let dir = UnixHost::resolve_on_path("sh").expect("sh must be on PATH");
        assert!(dir.exists(), "resolved dir {dir:?} should exist");
    }
}

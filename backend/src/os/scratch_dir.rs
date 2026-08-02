use crate::errors::{AppError, AppResult};
use crate::os::{EngineHost, Host};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh scratch directory under the system temp dir, removed on drop. Avoids a
/// `tempfile` production dependency; uniqueness comes from pid + a process-lifetime
/// counter, so two dirs from the same process never collide and two processes never
/// share one.
///
/// `prefix` namespaces the owner (`qm-mcp-world`, `qm-cert`, …) so each caller's
/// sweep only ever considers its own directories.
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create `<temp>/<prefix>-<pid>-<n>`, first sweeping this prefix's orphans.
    pub fn new(prefix: &'static str) -> AppResult<ScratchDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        sweep_orphans(prefix);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| AppError::Io(e.to_string()))?;
        Ok(ScratchDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// True only when we can prove the directory is ours to delete.
///
/// The system temp dir is shared between users on Linux, so `<prefix>-<pid>-<n>`
/// from *another* user can sit beside ours and its pid can collide with a dead pid
/// of ours. Deleting it would destroy a live run belonging to someone else, so
/// ownership is checked before liveness. Non-unix has a per-user temp dir, so the
/// check is vacuously true there.
#[cfg(unix)]
fn owned_by_us(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).map(|m| m.uid() == unsafe_uid()).unwrap_or(false)
}

#[cfg(unix)]
fn unsafe_uid() -> u32 {
    // `id -u` rather than a libc call: the crate is `#![deny(unsafe_code)]` and we
    // will not add a libc dependency for one number. The sweep is best-effort, so a
    // failed probe yields a uid that matches nothing and simply skips sweeping.
    Host::command("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

#[cfg(not(unix))]
fn owned_by_us(_path: &Path) -> bool {
    true
}

/// Best-effort sweep of `<prefix>-<pid>-*` dirs whose owning process is dead — a
/// SIGKILL'd run can never `Drop`, so without this every hard kill leaks a temp dir
/// forever. Errors are ignored: a sweep must never block a new run.
///
/// Liveness goes through `Host::pid_alive`, which is implemented on all three
/// platforms. The previous version shelled out to `kill -0` under `#[cfg(unix)]`
/// only, so **Windows leaked every orphaned directory**.
fn sweep_orphans(prefix: &'static str) {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    let stem = format!("{prefix}-");
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(&stem) else { continue };
        let Some(pid_str) = rest.split('-').next() else { continue };
        let Ok(pid) = pid_str.parse::<u32>() else { continue };
        if pid == me {
            continue; // our own live scratch dirs
        }
        let path = e.path();
        if owned_by_us(&path) && !Host::pid_alive(pid) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
#[path = "scratch_dir_tests.rs"]
mod tests;

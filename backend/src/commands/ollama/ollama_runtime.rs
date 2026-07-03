use crate::os::{EngineHost, Host};
use reqwest::Client;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, Signal, System};

pub const OLLAMA_TAGS_URL: &str = "http://localhost:11434/api/tags";
pub const READY_TIMEOUT_SECS: u64 = 10;
pub const POLL_INTERVAL_MS: u64 = 500;
pub const PROBE_TIMEOUT_MS: u64 = 1000;
/// Grace given to a graceful stop before we escalate to a hard kill — short
/// enough not to stall shutdown, long enough for a clean stop.
const KILL_GRACE_MS: u64 = 600;
const KILL_POLL_MS: u64 = 50;

pub async fn is_reachable(timeout_ms: u64) -> bool {
    let client = match Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(OLLAMA_TAGS_URL)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Locate the `ollama` binary. First tries PATH (`which` on Unix, `where.exe`
/// on Windows via `Host::resolve_on_path`); then falls back to each OS's
/// well-known install prefix. A GUI-launched Tauri app doesn't inherit the
/// shell PATH, so the fallbacks are load-bearing.
pub fn resolve_ollama() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) { "ollama.exe" } else { "ollama" };
    if let Some(parent) = Host::resolve_on_path(bin_name) {
        let p = parent.join(bin_name);
        if p.exists() {
            return Some(p);
        }
    }
    for candidate in ollama_fallback_paths() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn ollama_fallback_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/opt/homebrew/bin/ollama"),
            PathBuf::from("/usr/local/bin/ollama"),
        ]
    }
    #[cfg(target_os = "windows")]
    {
        let mut v = Vec::new();
        if let Ok(lad) = std::env::var("LOCALAPPDATA") {
            v.push(PathBuf::from(&lad).join("Programs").join("Ollama").join("ollama.exe"));
        }
        v.push(PathBuf::from(r"C:\Program Files\Ollama\ollama.exe"));
        v
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        vec![
            PathBuf::from("/usr/local/bin/ollama"),
            PathBuf::from("/usr/bin/ollama"),
        ]
    }
}

/// Whether we can auto-launch `ollama serve` *on this machine, right now*.
/// Runtime shift (Phase 2 reconciliation): main previously encoded this as a
/// compile-time `cfg!(target_os = "macos")` const because `spawn_serve` was
/// macOS-only. Now that `spawn_serve` is portable via `Host::apply_spawn_flags`,
/// the honest signal is "does `resolve_ollama` find an executable on disk" —
/// which is what actually gates whether we can spawn. On a Windows/Linux box
/// with Ollama installed, this returns true and the frontend shows Start
/// Ollama; on a box without Ollama installed, it returns false and
/// `ManualStartRequired` steers the user to the per-OS install command. No
/// more misreporting "not installed" on a Windows box that *has* Ollama.
pub fn auto_start_supported() -> bool {
    resolve_ollama().is_some()
}

/// Spawn `ollama serve` — portable across OSes. R1 spawn flags applied on
/// Windows so the child is in its own process group; killing this PID cleanly
/// stops the whole Ollama sidecar tree instead of QuantaMind itself.
pub fn spawn_serve(bin: &PathBuf) -> Result<u32, String> {
    let mut cmd = Host::command(bin);
    cmd.arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().map(|c| c.id()).map_err(|e| e.to_string())
}

pub async fn wait_until_ready() -> bool {
    let attempts = (READY_TIMEOUT_SECS * 1000) / POLL_INTERVAL_MS;
    for _ in 0..attempts {
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        if is_reachable(PROBE_TIMEOUT_MS).await {
            return true;
        }
    }
    false
}

/// Kill ALL `ollama serve` processes (regardless of who spawned them). Cross-OS
/// via `sysinfo` — replaces the macOS-only `pkill -f "ollama serve"`. Matching
/// checks the process's command-line for both the `ollama` binary AND the
/// `serve` argument, so a process named `ollama` running `pull` isn't touched.
pub fn kill_serve() -> Result<(), String> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    for proc in sys.processes().values() {
        let name_lc = proc.name().to_string_lossy().to_ascii_lowercase();
        let is_ollama_exe = name_lc == "ollama" || name_lc == "ollama.exe";
        if !is_ollama_exe {
            continue;
        }
        let has_serve_arg =
            proc.cmd().iter().any(|s| s.to_string_lossy().eq_ignore_ascii_case("serve"));
        if has_serve_arg && proc.kill_with(Signal::Term).is_none() {
            proc.kill();
        }
    }
    Ok(())
}

/// Kill the **specific** `ollama serve` PID this app spawned — targeted, so a
/// user's pre-existing daemon (never spawned by us) is left untouched. Graceful
/// stop first (`Host::graceful_stop` → SIGTERM on Unix, CTRL_BREAK_EVENT to the
/// child's own process group on Windows — see R1). If the process is still
/// alive after `KILL_GRACE_MS`, force a hard stop (`TerminateProcess` /
/// SIGKILL). An already-gone PID is success: the caller wanted it stopped.
pub fn kill_pid(pid: u32) -> Result<(), String> {
    let _ = Host::graceful_stop(pid);
    let attempts = KILL_GRACE_MS / KILL_POLL_MS;
    for _ in 0..attempts {
        if !Host::pid_alive(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(KILL_POLL_MS));
    }
    if Host::pid_alive(pid) {
        Host::hard_stop(pid)?;
    }
    Ok(())
}

// These tests spawn `true` — a POSIX-only builtin. The behaviour they verify
// (Host::pid_alive tracking a real lifecycle; kill_pid idempotent on a dead
// pid) is exercised on Windows by `os::windows::tests` (`pid_alive` against
// our own PID + a bogus PID), so the Windows path is not untested — it just
// uses the platform tests rather than spawning a shell builtin here.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn pid_alive_tracks_a_real_process_lifecycle() {
        assert!(Host::pid_alive(std::process::id()), "our own process is alive");
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!Host::pid_alive(pid), "a reaped child's pid is not alive");
    }

    #[test]
    fn kill_pid_on_an_already_dead_pid_is_ok() {
        // The reap path is idempotent: stopping a process that already exited succeeds
        // (the caller only wanted it gone) and never blocks the full grace window.
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(kill_pid(pid).is_ok());
    }
}

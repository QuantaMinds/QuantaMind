use reqwest::Client;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

pub const OLLAMA_TAGS_URL: &str = "http://localhost:11434/api/tags";
pub const READY_TIMEOUT_SECS: u64 = 10;
pub const POLL_INTERVAL_MS: u64 = 500;
pub const PROBE_TIMEOUT_MS: u64 = 1000;

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

#[cfg(target_os = "macos")]
pub fn resolve_ollama() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("ollama").output() {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(&path);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    for candidate in ["/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
pub fn resolve_ollama() -> Option<PathBuf> { None }

/// Whether this OS's runtime can auto-launch `ollama serve`. False everywhere
/// except macOS — callers use this to short-circuit *before* asking
/// `resolve_ollama()`, which otherwise conflates "not installed" with
/// "installed but this OS can't launch it" (see `ManualStartRequired`).
pub const AUTO_START_SUPPORTED: bool = cfg!(target_os = "macos");

pub const UNSUPPORTED_OS_MSG: &str =
    "Auto-start of Ollama is not yet supported on this OS — please start Ollama manually.";

#[cfg(target_os = "macos")]
pub fn spawn_serve(bin: &PathBuf) -> Result<u32, String> {
    Command::new(bin)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|c| c.id())
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn spawn_serve(_bin: &PathBuf) -> Result<u32, String> {
    Err(UNSUPPORTED_OS_MSG.into())
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

#[cfg(target_os = "macos")]
pub fn kill_serve() -> Result<(), String> {
    // pkill exit code 1 means "no process matched" — treat as success
    // (caller wanted Ollama stopped; it already is).
    Command::new("pkill").arg("-f").arg("ollama serve")
        .status().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn kill_serve() -> Result<(), String> { Err(UNSUPPORTED_OS_MSG.into()) }

/// Total grace given to a SIGTERM'd `ollama serve` to exit before we escalate to
/// SIGKILL — short enough not to stall app shutdown, long enough for a clean stop.
#[cfg(target_os = "macos")]
const KILL_GRACE_MS: u64 = 600;
#[cfg(target_os = "macos")]
const KILL_POLL_MS: u64 = 50;

/// True while `pid` still exists. `kill -0` signals nothing but returns success only
/// when the process is alive and signalable — the standard liveness probe.
#[cfg(target_os = "macos")]
fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Kill the **specific** `ollama serve` PID this app spawned — targeted, so a
/// user's pre-existing daemon (which we never started) is left untouched. SIGTERM
/// first (let it shut down cleanly), then escalate to SIGKILL if it's still alive
/// after a short grace, so the process can't survive app close. An already-gone PID
/// is success: the caller wanted it stopped.
#[cfg(target_os = "macos")]
pub fn kill_pid(pid: u32) -> Result<(), String> {
    // stderr silenced: a "no such process" on an already-gone PID is success here, not noise.
    Command::new("kill").arg(pid.to_string()).stderr(Stdio::null()).status().map_err(|e| e.to_string())?;
    let attempts = KILL_GRACE_MS / KILL_POLL_MS;
    for _ in 0..attempts {
        if !pid_alive(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(KILL_POLL_MS));
    }
    if pid_alive(pid) {
        // Graceful stop didn't take — force it so Ollama never outlives the app.
        Command::new("kill").arg("-9").arg(pid.to_string()).stderr(Stdio::null()).status().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn kill_pid(_pid: u32) -> Result<(), String> { Err(UNSUPPORTED_OS_MSG.into()) }

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn pid_alive_tracks_a_real_process_lifecycle() {
        assert!(pid_alive(std::process::id()), "our own process is alive");
        // Spawn and reap a trivial child; once reaped its pid reports dead.
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!pid_alive(pid), "a reaped child's pid is not alive");
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

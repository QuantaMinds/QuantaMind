use crate::commands::ollama::ollama_runtime::{
    auto_start_supported, is_reachable, kill_pid, kill_serve, resolve_ollama, spawn_serve,
    wait_until_ready, PROBE_TIMEOUT_MS,
};
use crate::errors::AppError;
use crate::sync::MutexExt;
use serde::Serialize;
use std::sync::Mutex;

pub const INSTALL_URL: &str = "https://ollama.com/download";
pub const READY_TIMEOUT_MSG: &str =
    "Ollama started but didn't become reachable within 10 seconds.";

#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OllamaStartResult {
    AlreadyRunning,
    Started { pid: u32 },
    NotInstalled { install_url: String },
    StartFailed { error: String },
    /// **Unreachable in practice as of Phase 2 (runtime-contract shift).** Kept
    /// for wire-format compatibility with `frontend/…/ollama_start.ts` (main's
    /// discriminated union still lists it) — the frontend can still render a
    /// "start Ollama yourself" flow if this ever surfaces from a future
    /// non-macOS path where `resolve_ollama` succeeds but auto-spawn is
    /// deliberately declined. Not emitted by the current `start_ollama_inner`.
    ManualStartRequired { install_url: String },
}

#[derive(Default)]
pub struct OllamaStartState {
    in_progress: Mutex<bool>,
    /// PID of an `ollama serve` **this app spawned** — `None` when Ollama was
    /// already running (a user's own daemon) so we never kill what we didn't start.
    started_pid: Mutex<Option<u32>>,
}

impl OllamaStartState {
    fn remember(&self, pid: u32) {
        *self.started_pid.lock_recover() = Some(pid);
    }

    /// Stop the **app-spawned** Ollama (if any) and forget it. Idempotent —
    /// used by `stop_ollama`, the exit reap, and the signal reaper. A pre-existing
    /// user daemon is left untouched.
    pub fn stop_owned(&self) -> Result<(), String> {
        if let Some(pid) = self.started_pid.lock_recover().take() {
            kill_pid(pid)?;
        }
        Ok(())
    }
}

/// Backstop: reap the app-spawned Ollama if the managed state is torn down.
impl Drop for OllamaStartState {
    fn drop(&mut self) {
        let _ = self.stop_owned();
    }
}

#[tauri::command]
pub async fn start_ollama(
    state: tauri::State<'_, OllamaStartState>,
) -> Result<OllamaStartResult, AppError> {
    {
        let mut g = state.in_progress.lock_recover();
        if *g {
            return Ok(OllamaStartResult::AlreadyRunning);
        }
        *g = true;
    }
    let result = start_ollama_inner().await;
    // Track only an Ollama WE spawned — `AlreadyRunning` means a user daemon we
    // must never reap. The exit/signal reaper kills this pid on app close.
    if let OllamaStartResult::Started { pid } = &result {
        state.remember(*pid);
    }
    *state.in_progress.lock_recover() = false;
    Ok(result)
}

async fn start_ollama_inner() -> OllamaStartResult {
    if is_reachable(PROBE_TIMEOUT_MS).await {
        return OllamaStartResult::AlreadyRunning;
    }
    // Runtime contract (Phase 2): the "can we auto-launch?" gate is the same
    // condition as "is Ollama on disk?" — `auto_start_supported()` is defined
    // as `resolve_ollama().is_some()`. So a single `resolve_ollama` check
    // gives the natural mapping: found → try spawn (StartFailed if it fails);
    // not found → NotInstalled with install URL. The old macOS-only
    // `!AUTO_START_SUPPORTED → ManualStartRequired` early-return is dropped
    // because it was gated on the compile-time const that no longer exists.
    let Some(bin) = resolve_ollama() else {
        return OllamaStartResult::NotInstalled { install_url: INSTALL_URL.into() };
    };
    let pid = match spawn_serve(&bin) {
        Ok(pid) => pid,
        Err(error) => return OllamaStartResult::StartFailed { error },
    };
    if wait_until_ready().await {
        OllamaStartResult::Started { pid }
    } else {
        OllamaStartResult::StartFailed { error: READY_TIMEOUT_MSG.into() }
    }
}

#[tauri::command]
pub async fn stop_ollama() -> Result<(), AppError> {
    kill_serve().map_err(AppError::Internal)
}

/// Whether we can auto-launch Ollama *on this machine, right now* — lets the
/// UI hide/relabel the "Start Ollama" button before the user ever clicks it,
/// rather than only reacting to a `NotInstalled` / `ManualStartRequired`
/// result after the fact. Delegates to `auto_start_supported()` (runtime
/// contract: `resolve_ollama().is_some()`), so a Windows/Linux user with
/// Ollama installed sees "Start Ollama"; a machine without Ollama sees
/// "Install Ollama" via main's per-OS empty-state UX.
#[tauri::command]
pub fn ollama_auto_start_supported() -> bool {
    auto_start_supported()
}

#[cfg(test)]
#[path = "ollama_start_tests.rs"]
mod tests;

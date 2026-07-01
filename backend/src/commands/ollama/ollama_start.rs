use crate::commands::ollama::ollama_runtime::{
    is_reachable, kill_pid, kill_serve, resolve_ollama, spawn_serve, wait_until_ready,
    AUTO_START_SUPPORTED, PROBE_TIMEOUT_MS,
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
    /// This OS can't auto-launch Ollama (non-macOS today), independent of
    /// whether the binary is actually installed — distinct from
    /// `NotInstalled` so the UI doesn't tell an installed-but-not-running
    /// Windows/Linux user that Ollama is missing.
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
    if !AUTO_START_SUPPORTED {
        return OllamaStartResult::ManualStartRequired { install_url: INSTALL_URL.into() };
    }
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

/// Whether this OS can auto-launch Ollama — lets the UI hide/relabel the
/// "Start Ollama" button before the user ever clicks it, rather than only
/// reacting to a `ManualStartRequired` result after the fact.
#[tauri::command]
pub fn ollama_auto_start_supported() -> bool {
    AUTO_START_SUPPORTED
}

#[cfg(test)]
#[path = "ollama_start_tests.rs"]
mod tests;

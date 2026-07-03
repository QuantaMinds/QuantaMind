use crate::commands::stt::stt_runtime::kill_server;
use crate::sync::MutexExt;
use serde::Serialize;
use std::collections::VecDeque;
use std::process::Child;
use std::sync::{Arc, Mutex};

/// Outcome of `start_whisper_server`, tagged by `status` so the frontend
/// branches without positional decoding (mirrors `LlamaStartResult`). The
/// gating variants each carry a `note` telling the user exactly what to fix.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SttStartResult {
    AlreadyRunning,
    Started { pid: u32, port: u16 },
    NotBundled { note: String },
    ModelMissing { note: String },
    VadMissing { note: String },
    /// A foreign process holds the STT port — fatal, never adopted (R2).
    PortConflict { note: String },
    StartFailed { error: String, stderr_tail: String },
}

pub struct Running {
    pub child: Child,
    /// Absolute path passed as `-m` — also the ownership/identity key.
    pub model_path: String,
    pub vad_path: String,
    pub tail: Arc<Mutex<VecDeque<String>>>,
}

/// The single app-managed `whisper-server`. A new model stops the previous one
/// (`future-considerations.md` tracks multi-server).
#[derive(Default)]
pub struct SttServerState {
    inner: Mutex<Option<Running>>,
}

impl SttServerState {
    pub fn is_model(&self, model_path: &str) -> bool {
        self.inner.lock_recover().as_ref().is_some_and(|r| r.model_path == model_path)
    }

    /// Whether *our* stored child is still alive. This is the R2 ownership truth:
    /// the port being reachable is not enough — a reachable port with no live
    /// child of ours is a foreign process, not our server.
    pub fn is_alive(&self) -> bool {
        let mut guard = self.inner.lock_recover();
        match guard.as_mut() {
            Some(r) => matches!(r.child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// The model path of the live server, or `None` when nothing is running —
    /// used by the transcribe command to label the artifact + confirm readiness.
    pub fn running_model(&self) -> Option<String> {
        let mut guard = self.inner.lock_recover();
        let r = guard.as_mut()?;
        matches!(r.child.try_wait(), Ok(None)).then(|| r.model_path.clone())
    }

    pub fn store(
        &self,
        child: Child,
        model_path: String,
        vad_path: String,
        tail: Arc<Mutex<VecDeque<String>>>,
    ) {
        *self.inner.lock_recover() = Some(Running { child, model_path, vad_path, tail });
    }

    /// The captured stderr tail (death diagnosis), or empty if nothing running.
    pub fn tail_snapshot(&self) -> String {
        self.inner
            .lock_recover()
            .as_ref()
            .map(|r| r.tail.lock_recover().iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }

    /// Kill and forget the running server, if any. Idempotent — used by `stop`,
    /// model switch, and the app-exit reap hook. Graceful-then-hard via
    /// `kill_server`.
    pub fn stop(&self) -> Result<(), String> {
        let running = self.inner.lock_recover().take();
        if let Some(mut r) = running {
            kill_server(&mut r.child)?;
        }
        Ok(())
    }
}

/// Backstop: `std::process::Child` detaches (does not kill) on drop, so reap
/// explicitly when the managed state is torn down. The exit hook in `lib.rs` is
/// the primary path; this covers other teardown (mirrors `MlxServerState`).
impl Drop for SttServerState {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)] // test fixtures spawn powershell/cmd/sh directly
    use super::*;
    use std::process::{Command, Stdio};

    fn empty_tail() -> Arc<Mutex<VecDeque<String>>> {
        Arc::new(Mutex::new(VecDeque::new()))
    }

    // Portable child helpers so the state-management tests below run on all
    // three CI matrix runners (macos-14 / ubuntu-22.04 / windows-latest), not
    // just POSIX. On Windows `sleep`/`true` don't exist — mirror the Phase 5
    // `spawn_spin_child_for_secs` idiom (PowerShell `Start-Sleep` reliably
    // blocks under redirected stdio; `cmd /c exit` immediately exits).
    fn spawn_sleep() -> Child {
        #[cfg(windows)]
        {
            Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
        #[cfg(not(windows))]
        {
            Command::new("sh")
                .args(["-c", "sleep 30"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
    }

    fn spawn_immediate_exit() -> Child {
        #[cfg(windows)]
        {
            Command::new("cmd")
                .args(["/c", "exit"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
        #[cfg(not(windows))]
        {
            Command::new("true")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        }
    }

    #[test]
    fn store_then_is_model_and_is_alive_track_the_running_child() {
        let state = SttServerState::default();
        assert!(!state.is_alive(), "nothing stored");
        assert!(!state.is_model("/m/ggml-tiny.en.bin"));

        state.store(spawn_sleep(), "/m/ggml-tiny.en.bin".into(), "/m/vad.bin".into(), empty_tail());
        assert!(state.is_model("/m/ggml-tiny.en.bin"));
        assert!(!state.is_model("/m/other.bin"));
        assert!(state.is_alive(), "the stored sleep is still running");

        assert!(state.stop().is_ok());
        assert!(!state.is_alive(), "stopped");
        assert!(state.stop().is_ok(), "stop is idempotent");
    }

    #[test]
    fn is_alive_is_false_once_the_child_exits() {
        let state = SttServerState::default();
        let quick = spawn_immediate_exit();
        state.store(quick, "/m/x.bin".into(), "/m/vad.bin".into(), empty_tail());
        // `cmd /c exit` on Windows takes ~30-80ms to fully spawn+reap; give
        // it a slightly longer grace than the POSIX `true` needed.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!state.is_alive(), "a child that exited on its own is not alive");
        let _ = state.stop();
    }

    #[test]
    fn start_result_serializes_with_snake_case_status_tags() {
        let cases = [
            (SttStartResult::AlreadyRunning, "already_running"),
            (SttStartResult::Started { pid: 42, port: 8093 }, "started"),
            (SttStartResult::NotBundled { note: "n".into() }, "not_bundled"),
            (SttStartResult::ModelMissing { note: "n".into() }, "model_missing"),
            (SttStartResult::VadMissing { note: "n".into() }, "vad_missing"),
            (SttStartResult::PortConflict { note: "n".into() }, "port_conflict"),
            (SttStartResult::StartFailed { error: "e".into(), stderr_tail: "t".into() }, "start_failed"),
        ];
        for (variant, tag) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert!(json.contains(&format!("\"status\":\"{tag}\"")), "got {json}");
        }
    }
}

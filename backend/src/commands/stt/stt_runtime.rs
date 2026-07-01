use crate::commands::stt::stt_stderr::spawn_stderr_reader;
use crate::inference::backend::endpoint;
use reqwest::{Client, StatusCode};
use std::collections::VecDeque;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fixed STT port — 8093, clear of MLX's 8082..=8092 scan range (see
/// `inference::backend::endpoint`).
pub const PORT: u16 = 8093;
pub const READY_TIMEOUT_SECS: u64 = 30;
pub const POLL_INTERVAL_MS: u64 = 500;
pub const PROBE_TIMEOUT_MS: u64 = 1000;
/// Grace period for a SIGTERM to land before the hard kill (R4).
const GRACEFUL_WAIT_MS: u64 = 2000;

fn client(timeout_ms: u64) -> Option<Client> {
    Client::builder().timeout(Duration::from_millis(timeout_ms)).build().ok()
}

async fn health_status(base: &str, timeout_ms: u64) -> Option<StatusCode> {
    let c = client(timeout_ms)?;
    c.get(format!("{base}/health")).send().await.ok().map(|r| r.status())
}

/// Readiness gate: `GET /health` returns **HTTP 200** only once whisper-server
/// has loaded the model (it answers 503 `{"status":"loading model"}` while
/// loading). Pure over `base` so it can be asserted against a mock server.
pub(crate) async fn ready_at(base: &str, timeout_ms: u64) -> bool {
    health_status(base, timeout_ms).await.map(|s| s.is_success()).unwrap_or(false)
}

/// Liveness: *any* HTTP answer (incl. 503-loading) means a server is bound to
/// the port; only a transport error means down. Used by the R2 collision check
/// to tell "up but loading" from "nothing there".
pub(crate) async fn reachable_at(base: &str, timeout_ms: u64) -> bool {
    health_status(base, timeout_ms).await.is_some()
}

/// True once the model is loaded and `/health` is 200.
pub async fn is_ready(timeout_ms: u64) -> bool {
    ready_at(endpoint::WHISPER_SERVER, timeout_ms).await
}

/// True if anything is answering on the STT port (ready or still loading).
pub async fn is_reachable(timeout_ms: u64) -> bool {
    reachable_at(endpoint::WHISPER_SERVER, timeout_ms).await
}

/// Arguments to launch `whisper-server` for one model + VAD on a fixed port.
/// Pure, so it can be asserted without spawning. Flags verified against
/// whisper.cpp `examples/server/server.cpp`.
pub fn build_spawn_args(model_path: &str, vad_path: &str, port: u16) -> Vec<String> {
    vec![
        "-m".into(), model_path.into(),
        "--host".into(), "127.0.0.1".into(),
        "--port".into(), port.to_string(),
        "--vad".into(),
        "--vad-model".into(), vad_path.into(),
    ]
}

/// The `whisper-server` executable file name for this platform.
pub fn bin_name() -> &'static str {
    if cfg!(windows) { "whisper-server.exe" } else { "whisper-server" }
}

/// Spawn `whisper-server` from `dir` (binary + dylibs colocated; `current_dir`
/// + per-OS lib-path envs from `Host::envs_for_lib_dir` resolve `@loader_path`
/// libs on macOS/Linux; Windows finds DLLs next to the .exe without any env).
/// stderr is piped into a tail ring for crash diagnosis; stdout/stdin nulled.
/// Returns the child (caller owns its lifecycle) and the shared tail.
pub fn spawn_server(
    dir: &Path,
    args: &[String],
) -> Result<(Child, Arc<Mutex<VecDeque<String>>>), String> {
    use crate::os::{EngineHost, Host};
    let mut cmd = Command::new(dir.join(bin_name()));
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in Host::envs_for_lib_dir(dir) {
        cmd.env(k, v);
    }
    // R1 spawn flags on Windows (CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP).
    Host::apply_spawn_flags(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    if let Some(stderr) = child.stderr.take() {
        spawn_stderr_reader(stderr, Arc::clone(&tail));
    }
    Ok((child, tail))
}

/// Terminate the running server. Graceful first (R4): request a stop via
/// `Host::graceful_stop` (SIGTERM on Unix, `GenerateConsoleCtrlEvent
/// (CTRL_BREAK_EVENT, pid)` on Windows — safe only because `apply_spawn_flags`
/// put each child in its own process group), poll `pid_alive` for up to 2 s,
/// then hard-kill through the `Child` handle. The server only *reads* model
/// files, so a hard kill can never corrupt weights — graceful is purely for
/// clean teardown. Idempotent: killing an already-exited child is success.
pub fn kill_server(child: &mut Child) -> Result<(), String> {
    use crate::os::{EngineHost, Host};
    if matches!(child.try_wait(), Ok(Some(_))) {
        return Ok(());
    }
    let _ = Host::graceful_stop(child.id());
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(GRACEFUL_WAIT_MS) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    match child.kill() {
        Ok(()) => {
            let _ = child.wait();
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn build_spawn_args_has_model_host_port_and_vad_in_order() {
        let args = build_spawn_args("/m/ggml-tiny.en.bin", "/m/ggml-silero-v6.2.0.bin", 8093);
        assert_eq!(
            args,
            vec![
                "-m",
                "/m/ggml-tiny.en.bin",
                "--host",
                "127.0.0.1",
                "--port",
                "8093",
                "--vad",
                "--vad-model",
                "/m/ggml-silero-v6.2.0.bin",
            ]
        );
    }

    #[test]
    fn bin_name_is_platform_specific() {
        let name = bin_name();
        if cfg!(windows) {
            assert_eq!(name, "whisper-server.exe");
        } else {
            assert_eq!(name, "whisper-server");
        }
    }

    #[tokio::test]
    async fn ready_only_on_200_not_503_or_refused() {
        let mut srv = Server::new_async().await;
        let _m = srv.mock("GET", "/health").with_status(200).create_async().await;
        assert!(ready_at(&srv.url(), 1000).await, "200 is ready");

        let mut loading = Server::new_async().await;
        let _l = loading.mock("GET", "/health").with_status(503).create_async().await;
        assert!(!ready_at(&loading.url(), 1000).await, "503 loading is not ready");

        assert!(!ready_at("http://127.0.0.1:1", 1000).await, "refused is not ready");
    }

    #[tokio::test]
    async fn reachable_on_200_and_503_but_not_refused() {
        let mut ok = Server::new_async().await;
        let _o = ok.mock("GET", "/health").with_status(200).create_async().await;
        assert!(reachable_at(&ok.url(), 1000).await);

        let mut loading = Server::new_async().await;
        let _l = loading.mock("GET", "/health").with_status(503).create_async().await;
        assert!(reachable_at(&loading.url(), 1000).await, "up but loading still counts as reachable");

        assert!(!reachable_at("http://127.0.0.1:1", 1000).await, "refused is not reachable");
    }

    #[cfg(unix)]
    #[test]
    fn kill_server_terminates_gracefully_and_is_idempotent() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // SIGTERM stops `sleep` well within the 2s grace window.
        assert!(kill_server(&mut child).is_ok());
        assert!(matches!(child.try_wait(), Ok(Some(_))), "child exited after kill");
        // A second kill on an already-exited child is a no-op success.
        assert!(kill_server(&mut child).is_ok());
    }
}

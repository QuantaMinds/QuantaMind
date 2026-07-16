//! stdio JSON-RPC transport for an MCP server subprocess.
//!
//! Spawns the server via `Host::command` (piped stdin/stdout, stderr drained to
//! a redacted tail), frames newline-delimited JSON, and correlates responses to
//! requests by id over `tokio::sync::oneshot`. Two real-world failure modes are
//! handled loudly rather than by hanging: a server that logs **non-JSON to
//! stdout** (recorded as "garbage" and reported in the request error), and a
//! server that **closes the connection** (sender dropped → a diagnostic carrying
//! the stderr tail).
//!
//! Process-group isolation and readiness gating are Phase 3 — `Host`'s spawn
//! flags are a no-op on Unix, so `kill` here is best-effort.

use crate::errors::{AppError, AppResult};
use crate::mcp::wire::{Notification, Request, RequestId, Response};
use crate::redact::redact_path;
use crate::sync::MutexExt;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

const STDERR_TAIL_CAP: usize = 20;
/// Cap on how much of a stray stdout line we quote back (avoid dumping a huge
/// non-JSON blob into an error).
const GARBAGE_QUOTE_CAP: usize = 200;
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Grace window between a group SIGTERM and the hard SIGKILL on teardown.
const GRACEFUL_WAIT: Duration = Duration::from_millis(1500);

type Pending = Arc<Mutex<HashMap<RequestId, oneshot::Sender<Response>>>>;
type Garbage = Arc<Mutex<Option<String>>>;

/// A live connection to one MCP server subprocess.
pub struct McpTransport {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicI64,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stdout_garbage: Garbage,
}

impl McpTransport {
    /// Spawn `program args…` as an MCP server over stdio.
    pub fn spawn(program: &str, args: &[String]) -> AppResult<McpTransport> {
        Self::spawn_with_env(program, args, &[])
    }

    /// Like [`spawn`], but sets extra environment variables on the child (their
    /// values come from the keychain, never from disk).
    pub fn spawn_with_env(
        program: &str,
        args: &[String],
        envs: &[(String, String)],
    ) -> AppResult<McpTransport> {
        use crate::os::{EngineHost, Host};
        let (prog, args) = resolve_spawn(program, args);
        let mut cmd = Host::command(&prog);
        cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in envs {
            cmd.env(k, v);
        }

        // Unix: make the child its OWN process-group leader (pgid == child pid)
        // via the SAFE stdlib API — so teardown kills the whole group and never
        // orphans a reparented `node` / forked worker. `apply_spawn_flags` is a
        // no-op on Unix, so this is where Unix group isolation happens; on
        // Windows the group comes from CREATE_NEW_PROCESS_GROUP in those flags.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn().map_err(|e| AppError::Io(redact_path(&e.to_string())))?;

        let stdin = child.stdin.take().ok_or_else(|| AppError::Internal("mcp: no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| AppError::Internal("mcp: no stdout".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| AppError::Internal("mcp: no stderr".into()))?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        let stdout_garbage: Garbage = Arc::new(Mutex::new(None));

        // stderr → redacted bounded tail (diagnosis only; never protocol).
        {
            let tail = Arc::clone(&stderr_tail);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut t = tail.lock_recover();
                    if t.len() >= STDERR_TAIL_CAP {
                        t.pop_front();
                    }
                    t.push_back(redact_path(&line));
                }
            });
        }

        // stdout → frame + correlate. On stream close, drop all senders so any
        // in-flight `request` awaits resolve to a "server closed" error.
        {
            let pending = Arc::clone(&pending);
            let garbage = Arc::clone(&stdout_garbage);
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        route_line(trimmed, &pending, &garbage);
                    }
                }
                pending.lock_recover().clear();
            });
        }

        Ok(McpTransport {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicI64::new(1),
            stderr_tail,
            stdout_garbage,
        })
    }

    /// Send a request and await its response with the default timeout.
    pub async fn request(&self, method: &str, params: Option<Value>) -> AppResult<Response> {
        self.request_timeout(method, params, DEFAULT_REQUEST_TIMEOUT).await
    }

    /// Send a request and await its correlated response, or a diagnostic on
    /// timeout / server-close.
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> AppResult<Response> {
        let id = RequestId::Num(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        self.pending.lock_recover().insert(id.clone(), tx);

        let req = Request::new(id.clone(), method, params);
        if let Err(e) = self.write_line(&serde_json::to_string(&req)?) {
            self.pending.lock_recover().remove(&id);
            return Err(e);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                self.pending.lock_recover().remove(&id);
                Err(AppError::Inference(self.closed_diagnostic(method)))
            }
            Err(_) => {
                self.pending.lock_recover().remove(&id);
                Err(AppError::Timeout(self.timeout_diagnostic(method)))
            }
        }
    }

    /// Send a notification (no id, never answered).
    pub fn notify(&self, method: &str, params: Option<Value>) -> AppResult<()> {
        let note = Notification::new(method, params);
        self.write_line(&serde_json::to_string(&note)?)
    }

    /// Terminate the server and its whole process group: graceful group-stop,
    /// a grace window, then a hard group-kill, then reap the `Child`.
    /// Idempotent — killing an already-exited child is a no-op.
    pub fn kill(&self) {
        let mut child = self.child.lock_recover();
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let pid = child.id();
        graceful_stop_group(pid);
        let start = std::time::Instant::now();
        while start.elapsed() < GRACEFUL_WAIT {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        hard_stop_group(pid);
        let _ = child.kill();
        let _ = child.wait();
    }

    /// The server process id (== its process-group id on Unix). For diagnostics
    /// and lifecycle tests.
    pub fn pid(&self) -> u32 {
        self.child.lock_recover().id()
    }

    /// Redacted last-N stderr lines (for diagnostics / tests).
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock_recover().iter().cloned().collect()
    }

    fn write_line(&self, line: &str) -> AppResult<()> {
        let mut stdin = self.stdin.lock_recover();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|e| AppError::Io(redact_path(&e.to_string())))
    }

    fn timeout_diagnostic(&self, method: &str) -> String {
        match self.stdout_garbage.lock_recover().clone() {
            Some(g) => format!(
                "MCP '{method}' timed out; the server wrote non-JSON to stdout \
                 (it may be logging to stdout instead of stderr): {g}"
            ),
            None => format!("MCP '{method}' timed out with no response"),
        }
    }

    fn closed_diagnostic(&self, method: &str) -> String {
        if let Some(g) = self.stdout_garbage.lock_recover().clone() {
            return format!(
                "MCP '{method}': server closed the connection after writing non-JSON \
                 to stdout (logging to stdout?): {g}"
            );
        }
        let tail = self.stderr_tail().join("\n");
        format!("MCP '{method}': server closed the connection. Last stderr:\n{tail}")
    }
}

impl Drop for McpTransport {
    fn drop(&mut self) {
        // A std::process::Child detaches (does not kill) on drop; kill so a
        // dropped transport can't leak a live server.
        self.kill();
    }
}

/// Resolve the program/args to actually spawn. On Windows a bare (extension-less,
/// non-absolute) program like `npx`/`npm`/`node` is a `.cmd` shim that bare
/// `CreateProcess` can't launch (`ENOENT`), so wrap it via `cmd /c …`. Elsewhere
/// (and for absolute/extensioned programs) launch directly.
fn resolve_spawn(program: &str, args: &[String]) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        let p = std::path::Path::new(program);
        if !p.is_absolute() && p.extension().is_none() {
            let mut wrapped = vec!["/c".to_string(), program.to_string()];
            wrapped.extend_from_slice(args);
            return ("cmd".to_string(), wrapped);
        }
    }
    (program.to_string(), args.to_vec())
}

/// The `kill(1)` argv that signals the process group `pgid` — and ONLY that group.
///
/// The `--` is load-bearing, not style. Without it, procps `kill` (every Linux distro)
/// reads the leading-dash pgid as bundled short options rather than a negative pid, and
/// signals **the caller's own process group** instead of the target. Measured on
/// ubuntu-22.04 (procps, `/bin/kill`), child correctly leading its own group:
///
/// ```text
///   child pid=4192  child pgid=4192  |  caller pgid=4191
///   /bin/kill -TERM -4192   →  caller killed by SIGTERM (exit 143); the group SURVIVED
///   /bin/kill -TERM -- -4192 →  exit 0; the group died; the caller lived
/// ```
///
/// So the old form did the exact opposite of its purpose: it orphaned the server it meant
/// to reap and killed the app that called it. It went unseen because BSD `kill` (macOS)
/// parses the bare form correctly; `--` is accepted by both, so one argv is right on both.
fn group_signal_argv(signal: &str, pgid: u32) -> Vec<String> {
    vec![signal.to_string(), "--".to_string(), format!("-{pgid}")]
}

/// Signal a whole process group, refusing the one target that can never be meant.
///
/// `pgid == 0` is the footgun this guards: `kill -- -0` is `kill(0, sig)`, POSIX for
/// "signal MY OWN process group" — it would kill QuantaMind (and, under a test runner, the
/// harness). A real child always has a non-zero pid, so 0 can only ever arrive from a bug;
/// signalling nothing is the only safe reading.
#[cfg(unix)]
fn signal_group(signal: &str, pgid: u32) {
    if pgid == 0 {
        return;
    }
    let _ = std::process::Command::new("kill")
        .args(group_signal_argv(signal, pgid))
        .stderr(Stdio::null())
        .status();
}

/// Ask the child's whole process group to exit cleanly. Unix: `kill -TERM -- -<pgid>`
/// (negative pid = the group; see [`group_signal_argv`] for why `--` is required).
/// Windows: `CTRL_BREAK` to the group via `Host` (the child leads a group from
/// `apply_spawn_flags`).
fn graceful_stop_group(pid: u32) {
    #[cfg(unix)]
    {
        signal_group("-TERM", pid);
    }
    #[cfg(not(unix))]
    {
        use crate::os::{EngineHost, Host};
        let _ = Host::graceful_stop(pid);
    }
}

/// Hard-kill the child's whole process group. Unix: `kill -KILL -- -<pgid>`.
/// Windows: no-op here — the direct-child `TerminateProcess` (`child.kill()`)
/// after this call is the hard stop, matching the existing sidecar pattern.
fn hard_stop_group(pid: u32) {
    #[cfg(unix)]
    {
        signal_group("-KILL", pid);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

/// Route one stdout line: a `{id, result|error}` response is correlated to its
/// waiter; a `{method}` (notification / server request) is ignored for now; a
/// non-JSON line is recorded as the first "garbage" (the #1 corruption bug).
fn route_line(line: &str, pending: &Pending, garbage: &Garbage) {
    let val: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            let mut g = garbage.lock_recover();
            if g.is_none() {
                *g = Some(redact_path(&line.chars().take(GARBAGE_QUOTE_CAP).collect::<String>()));
            }
            return;
        }
    };
    let has_id = val.get("id").map(|v| !v.is_null()).unwrap_or(false);
    let is_response = has_id && (val.get("result").is_some() || val.get("error").is_some());
    if is_response {
        if let Ok(resp) = serde_json::from_value::<Response>(val) {
            if let Some(tx) = pending.lock_recover().remove(&resp.id) {
                let _ = tx.send(resp);
            }
        }
    }
    // else: server→client notification/request — unhandled until a later phase.
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;

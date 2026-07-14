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
        use crate::os::{EngineHost, Host};
        let mut cmd = Host::command(program);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
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

    /// Best-effort terminate. P3 replaces this with graceful group-stop +
    /// readiness-aware reaping.
    pub fn kill(&self) {
        use crate::os::{EngineHost, Host};
        let mut child = self.child.lock_recover();
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = Host::graceful_stop(child.id());
        let _ = child.kill();
        let _ = child.wait();
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

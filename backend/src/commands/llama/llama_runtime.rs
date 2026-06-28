use crate::inference::backend::endpoint;
use reqwest::Client;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 8081, NOT 8080 — `mlx_lm.server`'s default is 8080, and a stray one there
/// would shadow our llama-server (health passes, inference 404s). See
/// `inference::backend::endpoint`.
pub const PORT: u16 = 8081;
pub const READY_TIMEOUT_SECS: u64 = 30;
pub const POLL_INTERVAL_MS: u64 = 500;
pub const PROBE_TIMEOUT_MS: u64 = 1000;

/// Health probe for the llama.cpp sidecar, in the shared `HealthStatus` shape the
/// Ollama/MLX probes return so the frontend can poll all three uniformly. No
/// version string (llama-server's `/health` reports none) → `version: None`.
#[tauri::command]
pub async fn check_llama_health() -> crate::commands::system::health::HealthStatus {
    crate::commands::system::health::HealthStatus {
        available: is_reachable(PROBE_TIMEOUT_MS).await,
        version: None,
    }
}

/// Probe the sidecar's `/health` endpoint.
pub async fn is_reachable(timeout_ms: u64) -> bool {
    let client = match Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(format!("{}/health", endpoint::LLAMA_SERVER))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Arguments to launch `llama-server` for one GGUF on a fixed port. Pure, so it
/// can be asserted without spawning a process.
///
/// `--jinja` makes the chat endpoint apply the GGUF's embedded chat template —
/// without it (and the `/v1/chat/completions` route in `inference::llama`) the
/// model never sees its trained turn structure, never emits EOS, and loops to
/// `n_predict`. `-c` pins the context window from the GGUF header so long
/// agentic transcripts don't silently overflow a too-small default.
///
/// `template_file` is an OPTIONAL `.jinja` override (`--chat-template-file`), used
/// only when a model's embedded template is broken (resolved by `llama_templates`).
/// `None` ⇒ the embedded template via `--jinja` — the default for every model.
pub fn build_spawn_args(gguf_path: &str, port: u16, ctx: u32, template_file: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        gguf_path.into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--jinja".into(),
        "-c".into(),
        ctx.to_string(),
    ];
    if let Some(path) = template_file {
        args.push("--chat-template-file".into());
        args.push(path.into());
    }
    args
}

/// Context window to launch with when the GGUF header omits one.
pub const DEFAULT_CONTEXT: u32 = 4096;

/// Upper bound on `-c`. The GGUF's declared `context_length` is the model's MAX
/// (e.g. gemma4 reports 262144 = 256K), and launching `llama-server -c <that>`
/// allocates a KV cache for the full window up front — 256K tokens for a 12B
/// model OOMs and llama-server dies with "Compute error". So `-c` is the GGUF
/// value CAPPED here: ample headroom for agentic transcripts (well above the old
/// 4096 default), small enough that the KV cache always allocates.
pub const MAX_CONTEXT: u32 = 8192;

/// One GGUF header read → the two values the spawn needs: the context window
/// (`-c`, capped — see `MAX_CONTEXT`) and the architecture string (the
/// chat-template override lookup key). Both degrade safely when the header can't
/// be read (`DEFAULT_CONTEXT`, empty arch).
pub fn spawn_meta(gguf_path: &str) -> (u32, String) {
    match crate::inference::gguf::gguf::inspect_gguf(Path::new(gguf_path)) {
        Ok(m) => (cap_context(m.context_length), m.architecture),
        Err(_) => (DEFAULT_CONTEXT, String::new()),
    }
}

/// The `-c` value: the GGUF's declared context, capped at `MAX_CONTEXT`; the
/// `DEFAULT_CONTEXT` floor when the header omits it. Pure, so the cap is tested
/// without a GGUF fixture.
pub fn cap_context(ctx: Option<u32>) -> u32 {
    ctx.unwrap_or(DEFAULT_CONTEXT).min(MAX_CONTEXT)
}

/// The `llama-server` executable file name for this platform.
pub fn bin_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Spawn `llama-server` from `dir` (which holds the binary and its dylibs),
/// returning the child so the caller owns its lifecycle. `current_dir` +
/// `DYLD_FALLBACK_LIBRARY_PATH` ensure the `@rpath`/`@loader_path` dylibs
/// resolve regardless of cwd. Killing by `Child` handle is portable across
/// macOS / Windows / Linux, unlike Ollama's macOS-only `pkill`.
///
/// stderr is `piped` (not discarded) so the caller can drain it for the death
/// diagnosis — e.g. a bundled binary too old for `--jinja` exits immediately,
/// and its stderr names the rejected flag.
pub fn spawn_server(dir: &Path, args: &[String]) -> Result<Child, String> {
    let mut cmd = Command::new(dir.join(bin_name()));
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(target_os = "macos")]
    cmd.env("DYLD_FALLBACK_LIBRARY_PATH", dir);
    #[cfg(target_os = "linux")]
    cmd.env("LD_LIBRARY_PATH", dir);
    cmd.spawn().map_err(|e| e.to_string())
}

const TAIL_CAP: usize = 20;

/// Drain the child's piped stderr on a background thread into a bounded tail
/// ring (last `TAIL_CAP` lines), returned for the death diagnosis. Draining is
/// mandatory: an undrained pipe fills and blocks the child forever. The thread
/// ends when the stream closes (process exit).
pub fn spawn_stderr_tail(stderr: ChildStderr) -> Arc<Mutex<VecDeque<String>>> {
    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_CAP)));
    let sink = tail.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut t = sink.lock().unwrap_or_else(|p| p.into_inner());
            if t.len() >= TAIL_CAP {
                t.pop_front();
            }
            t.push_back(line);
        }
    });
    tail
}

pub const JINJA_UNSUPPORTED_MSG: &str =
    "The bundled llama-server is too old for the --jinja flag (it rejected it on \
     startup). Rebuild/update the bundled binary so it supports --jinja.";

/// True when the captured stderr names `--jinja` as a rejected argument — the
/// signature of a stale binary. Matched loosely (llama.cpp's arg-parser wording
/// varies across builds): the flag name plus any rejection word.
pub fn jinja_unsupported(tail: &VecDeque<String>) -> bool {
    tail.iter().any(|line| {
        let l = line.to_ascii_lowercase();
        l.contains("jinja")
            && (l.contains("invalid")
                || l.contains("unknown")
                || l.contains("unrecognized")
                || l.contains("error"))
    })
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

/// Terminate the running server. Idempotent: killing an already-exited child is
/// treated as success (the caller wanted it stopped; it already is).
pub fn kill_server(child: &mut Child) -> Result<(), String> {
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
#[path = "llama_runtime_tests.rs"]
mod tests;

use crate::inference::backend::endpoint;
use crate::inference::vram_math::calculate_kv_cache_bytes;
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

/// Floor on `-c`: a usable minimum window even on a tight machine — the hardware
/// ceiling never clamps below this, and it doubles as the cliff-probe headroom.
pub const MIN_CONTEXT: u32 = 2048;

/// Percent of TOTAL RAM treated as usable for the model weights + KV cache; the rest
/// is reserved for the OS, this app, and other working sets. We budget against stable
/// CAPACITY rather than instantaneous free memory because the KV cache is pre-allocated
/// at spawn and persists — sizing it off momentary free RAM would make the launched
/// window swing with whatever else is open (and collapse to the floor under any
/// transient pressure). On unified memory ~70% is a safe working set.
const USABLE_MEMORY_PCT: u64 = 70;

/// `-c` is rounded down to this step so the launched window is a tidy number.
const CTX_STEP: u32 = 256;

/// Upper bound on `-c`. The GGUF's declared `context_length` is the model's MAX
/// (e.g. gemma4 reports 262144 = 256K), and launching `llama-server -c <that>`
/// allocates a KV cache for the full window up front — 256K tokens for a 12B
/// model OOMs and llama-server dies with "Compute error". So `-c` is the GGUF
/// value CAPPED here: ample headroom for agentic transcripts (well above the old
/// 4096 default), small enough that the KV cache always allocates.
pub const MAX_CONTEXT: u32 = 8192;

/// Transformer dims the KV-cache estimate needs (`vram_math`), read from the GGUF
/// header. `Some` only when every field is present, so the ceiling never divides by
/// a guessed dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KvDims {
    pub layers: u64,
    pub head_count: u64,
    pub head_count_kv: u64,
    pub embedding_length: u64,
}

/// One GGUF header read → the three things the spawn needs: the model's declared
/// context window (the RAW header value, `None` when absent — `resolve_launch_ctx`
/// turns it into the `-c` flag), the architecture string (the chat-template override
/// lookup key), and the KV dims (`Some` only when the header carries all four — used
/// to bound `-c` to RAM). All degrade safely when the header can't be read.
pub struct SpawnMeta {
    pub ctx: Option<u32>,
    pub arch: String,
    pub dims: Option<KvDims>,
}

pub fn spawn_meta(gguf_path: &str) -> SpawnMeta {
    match crate::inference::gguf::gguf::inspect_gguf(Path::new(gguf_path)) {
        Ok(m) => {
            let dims = match (m.block_count, m.head_count, m.head_count_kv, m.embedding_length) {
                (Some(layers), Some(head_count), Some(head_count_kv), Some(embedding_length)) => {
                    Some(KvDims { layers, head_count, head_count_kv, embedding_length })
                }
                _ => None,
            };
            SpawnMeta { ctx: m.context_length, arch: m.architecture, dims }
        }
        Err(_) => SpawnMeta { ctx: None, arch: String::new(), dims: None },
    }
}

/// The largest `-c` whose KV cache fits in the machine's usable RAM alongside the
/// model weights. Conservative by design: it takes `USABLE_MEMORY_PCT` of TOTAL RAM,
/// subtracts the on-disk weights, divides the remaining budget by the per-token KV
/// cost, floors to `CTX_STEP`, and never drops below `MIN_CONTEXT`. Budgeting on total
/// (not free) memory makes the ceiling a stable property of (machine, model) — the
/// same model probes to the same depth regardless of what else is open.
///
/// When the GGUF doesn't expose the dims we need (or per-token cost is zero) we CANNOT
/// measure a real ceiling, so we return `u32::MAX` — i.e. NO RAM clamp. This is the
/// safe direction: an unmeasurable ceiling must never silently cap the user's explicit
/// `num_ctx` (that would defeat an informed opt-in, e.g. gemma whose KV-heads are
/// array-typed). The unset-default 8K cap still applies via `cap_context`; only an
/// explicit, deliberate window is left unbounded here. Pure: the caller supplies total
/// memory, so it's tested without a live machine.
pub fn hardware_ctx_ceiling(model_bytes: u64, dims: Option<KvDims>, total_bytes: u64) -> u32 {
    let Some(d) = dims else { return u32::MAX };
    let per_token = calculate_kv_cache_bytes(d.layers, d.head_count, d.head_count_kv, d.embedding_length, 1);
    if per_token == 0 {
        return u32::MAX;
    }
    let usable = total_bytes / 100 * USABLE_MEMORY_PCT;
    let budget = usable.saturating_sub(model_bytes);
    let raw = (budget / per_token).min(u32::MAX as u64) as u32;
    (raw / CTX_STEP * CTX_STEP).max(MIN_CONTEXT)
}

/// The `-c` value: the GGUF's declared context, capped at `MAX_CONTEXT`; the
/// `DEFAULT_CONTEXT` floor when the header omits it. Pure, so the cap is tested
/// without a GGUF fixture.
pub fn cap_context(ctx: Option<u32>) -> u32 {
    ctx.unwrap_or(DEFAULT_CONTEXT).min(MAX_CONTEXT)
}

/// The `-c` value to launch `llama-server` with, given the GGUF's declared
/// context (`gguf_ctx`) and the user's optional `num_ctx` param.
///
/// llama.cpp fixes its context at launch (no per-request `n_ctx`), so the user's
/// "Context window" param can only take effect HERE. When set, it is honored —
/// an informed opt-in (the param tooltip warns about KV-cache memory) — bounded
/// by the model's own declared max so we never request more than the weights
/// support. When UNSET, the safe default applies (`cap_context`: the GGUF value
/// capped at `MAX_CONTEXT`) so a small machine never allocates a giant KV cache
/// by surprise. EITHER way the result is then bounded by `hw_ceiling` (what free
/// RAM holds) so even an explicit high `num_ctx` can't OOM the spawn, and floored
/// at `MIN_CONTEXT`. Pure, so it's tested without spawning a server.
pub fn resolve_launch_ctx(gguf_ctx: Option<u32>, requested: Option<u32>, hw_ceiling: u32) -> u32 {
    let base = match requested {
        Some(r) if r > 0 => match gguf_ctx {
            Some(max) => r.min(max),
            None => r,
        },
        _ => cap_context(gguf_ctx),
    };
    base.min(hw_ceiling).max(MIN_CONTEXT)
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
/// per-OS lib-path env vars (from `Host::envs_for_lib_dir`) ensure the
/// `@rpath` / `@loader_path` dylibs resolve regardless of cwd. Killing by
/// `Child` handle is portable across macOS / Windows / Linux, unlike Ollama's
/// macOS-only `pkill`.
///
/// stderr is `piped` (not discarded) so the caller can drain it for the death
/// diagnosis — e.g. a bundled binary too old for `--jinja` exits immediately,
/// and its stderr names the rejected flag.
pub fn spawn_server(dir: &Path, args: &[String]) -> Result<Child, String> {
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
    // R1: on Windows, sets CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP so a
    // subsequent `graceful_stop` targets the child (and its tree), not us.
    // No-op on Unix.
    Host::apply_spawn_flags(&mut cmd);
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

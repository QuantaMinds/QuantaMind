use crate::inference::backend::endpoint;
use crate::inference::vram_math::{calculate_kv_cache_bytes, kv_cache_bytes_at, KvPrecision};
use reqwest::Client;
use serde::Deserialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStderr, Stdio};
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

/// The subset of llama-server `/props` we surface: the loaded model's path and the
/// per-slot context window. Only these two fields — everything else is ignored.
#[derive(Deserialize)]
struct LlamaProps {
    /// Absolute GGUF path (recent llama.cpp). Older builds omit it, carrying the path
    /// in `default_generation_settings.model` instead — handled by the fallback below.
    #[serde(default)]
    model_path: String,
    #[serde(default)]
    default_generation_settings: PropsGenSettings,
}

#[derive(Deserialize, Default)]
struct PropsGenSettings {
    #[serde(default)]
    n_ctx: u32,
    #[serde(default)]
    model: String,
}

/// Extract `(model_path, n_ctx)` from a llama-server `/props` body. Prefers the top-level
/// `model_path`, falling back to `default_generation_settings.model` for older builds.
/// `None` when the body doesn't parse or carries no model path — never a fabricated entry.
/// Pure, so parsing is asserted without a live server.
pub fn parse_props(body: &str) -> Option<(String, u32)> {
    let props: LlamaProps = serde_json::from_str(body).ok()?;
    let path = if !props.model_path.is_empty() {
        props.model_path
    } else if !props.default_generation_settings.model.is_empty() {
        props.default_generation_settings.model
    } else {
        return None;
    };
    Some((path, props.default_generation_settings.n_ctx))
}

/// Probe the running llama-server's `/props` for `(model_path, n_ctx)` — the same shape
/// `LlamaServerState::running_summary` returns for an app-spawned server. Surfaces a server
/// the app did NOT start (a manual `llama-server`, or one the `qm` CLI launched), which the
/// app's own state knows nothing about. `None` when nothing is listening, `/props` fails, or
/// no model path is reported. Best-effort: every failure degrades to `None`.
pub async fn probe_running_model(timeout_ms: u64) -> Option<(String, u32)> {
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .ok()?;
    let resp = client
        .get(format!("{}/props", endpoint::LLAMA_SERVER))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_props(&resp.text().await.ok()?)
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
pub fn build_spawn_args(gguf_path: &str, port: u16, plan: &LaunchPlan, template_file: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-m".into(),
        gguf_path.into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--jinja".into(),
        "-c".into(),
        plan.ctx.to_string(),
    ];
    // Memory-safety flags, applied ONLY when the plan calls for them (a tight host that
    // couldn't otherwise hold the requested context). Flash Attention shrinks the attention
    // compute buffer; a Q8 KV cache halves the per-token KV memory — together they hold ~2×
    // the context in the same RAM and avert the Metal `kIOGPUCommandBufferCallbackErrorOutOfMemory`
    // wedge. A Q8 cache REQUIRES flash attention, so the two are set together (`plan` guarantees it).
    if plan.flash_attn {
        args.push("-fa".into());
        args.push("on".into());
    }
    if plan.kv == KvType::Q8 {
        args.push("-ctk".into());
        args.push("q8_0".into());
        args.push("-ctv".into());
        args.push("q8_0".into());
    }
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
    // The live launch path budgets on the total-RAM heuristic (no measured GPU limit here);
    // the ceiling METERS pass the measured working set through `ctx_ceilings` instead.
    ceiling_from_per_token(usable_memory_bytes(total_bytes, None), model_bytes, per_token)
}

/// The largest context this (machine, model) holds at each KV-cache precision —
/// the data behind the Latency tab's "context ceiling by KV precision" meters.
/// `None` for a precision means unmeasurable (unknown dims / zero per-token cost),
/// rendered "Not available" — never a fabricated ceiling. Q8 roughly doubles F16,
/// Q4 roughly quadruples it (modulo the `CTX_STEP` rounding). Q4 is PLANNING info
/// only: a real launch never auto-picks a Q4 cache (`KvType` has no Q4 arm).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CtxCeilings {
    pub f16: Option<u32>,
    pub q8: Option<u32>,
    pub q4: Option<u32>,
    /// Whether the WEIGHTS fit under the GPU's hard memory limit — the question the
    /// ceilings alone can't answer (a 100K ceiling is meaningless if the weights don't
    /// even load on the GPU). See `FitVerdict`.
    pub fit: FitVerdict,
}

/// Whether the model's weights fit under the GPU's hard memory limit. The context ceilings
/// say how much KV *could* fit; this says whether the model itself fits on the GPU at all —
/// on Apple Silicon a model above the Metal working set spills to CPU/swap and crawls
/// regardless of any ceiling. `Unknown` when the limit is unmeasured (off macOS, or no
/// measured working set): we never fabricate a fit verdict from the total-RAM heuristic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FitVerdict {
    /// Weights leave real headroom under the limit for a meaningful KV cache.
    Fits,
    /// Weights fit but occupy ≥ `TIGHT_FIT_PCT` of the limit — little room for context.
    Tight,
    /// Weights alone exceed the limit — can't stay resident on the GPU (CPU spill / swap).
    SpillsToCpu,
    /// The GPU memory limit is unmeasured — no honest verdict possible.
    Unknown,
}

/// Weights at or above this percent of the GPU limit ⇒ `Tight` (fits, but almost no room
/// left for the KV cache once the model is loaded).
const TIGHT_FIT_PCT: u64 = 85;

/// Classify how the weights sit against the GPU's hard memory limit. Uses the MEASURED
/// `working_set_bytes` only — `None` (unmeasured) or a zero model size yields `Unknown`
/// rather than a guessed verdict. Pure.
pub fn fit_verdict(model_bytes: u64, working_set_bytes: Option<u64>) -> FitVerdict {
    let (Some(limit), true) = (working_set_bytes, model_bytes > 0) else {
        return FitVerdict::Unknown;
    };
    if limit == 0 {
        return FitVerdict::Unknown;
    }
    if model_bytes >= limit {
        FitVerdict::SpillsToCpu
    } else if model_bytes * 100 >= limit * TIGHT_FIT_PCT {
        FitVerdict::Tight
    } else {
        FitVerdict::Fits
    }
}

/// Compute the three per-precision context ceilings plus the weights' fit verdict, from the
/// model's dims, this machine's total memory, and — on Apple Silicon — its MEASURED Metal
/// working-set limit (`working_set_bytes`). The ceilings budget against
/// `usable_memory_bytes(total, working_set)` so "fits" means fits on the GPU, not an
/// optimistic slice of total RAM. Pure (the caller supplies the memory figures) so it's
/// tested without a live machine. `u32::MAX` from `ceiling_from_per_token` (unmeasurable)
/// maps to `None`.
pub fn ctx_ceilings(model_bytes: u64, dims: KvDims, total_bytes: u64, working_set_bytes: Option<u64>) -> CtxCeilings {
    let usable = usable_memory_bytes(total_bytes, working_set_bytes);
    let ceiling_at = |p: KvPrecision| {
        let per_token = kv_cache_bytes_at(p, dims.layers, dims.head_count, dims.head_count_kv, dims.embedding_length, 1);
        let c = ceiling_from_per_token(usable, model_bytes, per_token);
        (c != u32::MAX).then_some(c)
    };
    CtxCeilings {
        f16: ceiling_at(KvPrecision::F16),
        q8: ceiling_at(KvPrecision::Q8),
        q4: ceiling_at(KvPrecision::Q4),
        fit: fit_verdict(model_bytes, working_set_bytes),
    }
}

/// The memory the GPU can actually use for the model weights + KV cache. On Apple Silicon
/// this is the MEASURED Metal working-set limit (`GpuInfo::gpu_working_set_bytes`, ~66-75%
/// of RAM — the point past which allocations get rejected); off macOS, or when unmeasured,
/// it falls back to `USABLE_MEMORY_PCT` of total RAM (the conservative heuristic). Budgeting
/// context against the measured cap makes "fits in memory" mean "fits on the GPU", not an
/// optimistic slice of total RAM the OS would never let the GPU wire down. Pure.
pub fn usable_memory_bytes(total_bytes: u64, working_set_bytes: Option<u64>) -> u64 {
    working_set_bytes.unwrap_or(total_bytes / 100 * USABLE_MEMORY_PCT)
}

/// The largest `-c` whose KV cache (at `per_token` bytes/token) fits the `usable` memory
/// budget alongside the weights. Extracted from `hardware_ctx_ceiling` so the Q8-KV plan can
/// pass HALF the per-token cost (a quantized cache) and get the correspondingly larger ceiling.
/// `per_token` of 0 (unknown dims) ⇒ no clamp (`u32::MAX`), the safe direction (never silently
/// cap an explicit window). The caller supplies `usable` (from `usable_memory_bytes`) so the
/// budget source — measured GPU limit vs total-RAM heuristic — is decided in one place. Pure.
fn ceiling_from_per_token(usable_bytes: u64, model_bytes: u64, per_token: u64) -> u32 {
    if per_token == 0 {
        return u32::MAX;
    }
    let budget = usable_bytes.saturating_sub(model_bytes);
    let raw = (budget / per_token).min(u32::MAX as u64) as u32;
    (raw / CTX_STEP * CTX_STEP).max(MIN_CONTEXT)
}

/// KV-cache element precision the launch will request. `F16` is llama.cpp's default (2 bytes
/// per element); `Q8` (1 byte) halves KV memory but REQUIRES flash attention, so a plan that
/// picks `Q8` always also sets `flash_attn`. Deliberately NARROWER than
/// `vram_math::KvPrecision`: the absence of a `Q4` variant is the type-level proof that a
/// launch never auto-picks a Q4 cache (real quality cost, and much slower at long context) —
/// Q4 exists only as planning math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KvType {
    F16,
    Q8,
}

impl KvType {
    pub fn precision(self) -> KvPrecision {
        match self {
            KvType::F16 => KvPrecision::F16,
            KvType::Q8 => KvPrecision::Q8,
        }
    }
}

/// The hardware-aware launch decision: the `-c` window, whether to force flash attention, the
/// KV-cache precision, and an OPTIONAL user-facing `note` explaining any memory constraint that
/// was applied (and how the server is now running safely). `note` is `Some` ONLY when a
/// constraint kicked in — a roomy machine launches exactly as before with no message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub ctx: u32,
    pub flash_attn: bool,
    pub kv: KvType,
    pub note: Option<String>,
}

/// Decide how to launch `llama-server` for THIS (model, machine): pick the largest safe context
/// and, when full-precision KV won't fit the desired window, enable flash attention + a Q8 KV
/// cache (halving KV memory, ~2× the reachable context) instead of silently shrinking the
/// window. Returns a user-facing `note` whenever it had to intervene, so the app can tell the
/// user what constraint was detected and how the server is running safely.
///
/// `desired` is the window the user asked for (their `num_ctx`, else the GGUF default capped at
/// `MAX_CONTEXT`), bounded by the model's own max. If it already fits full-precision KV, we
/// launch plainly (F16, no forced flags, no note). If it doesn't, Q8 KV raises the ceiling; we
/// take the min of `desired` and that Q8 ceiling — so even Q8 can't OOM the pre-allocated cache.
/// When dims/weights are unmeasurable we can't reason about memory, so we launch plainly at the
/// desired window (never fabricate a constraint). Pure: the caller supplies total memory.
pub fn plan_launch(
    model_bytes: Option<u64>,
    dims: Option<KvDims>,
    total_bytes: u64,
    gguf_ctx: Option<u32>,
    requested: Option<u32>,
) -> LaunchPlan {
    let desired = match requested {
        Some(r) if r > 0 => gguf_ctx.map_or(r, |max| r.min(max)),
        _ => cap_context(gguf_ctx),
    }
    .max(MIN_CONTEXT);

    // Without measurable weights AND dims we can't budget memory — launch plainly (legacy path).
    let (Some(mb), Some(d)) = (model_bytes, dims) else {
        return LaunchPlan { ctx: desired, flash_attn: false, kv: KvType::F16, note: None };
    };
    // Launch path budgets on the total-RAM heuristic (no measured GPU limit threaded here yet).
    let usable = usable_memory_bytes(total_bytes, None);
    let per_token_f16 = calculate_kv_cache_bytes(d.layers, d.head_count, d.head_count_kv, d.embedding_length, 1);
    let f16_ceiling = ceiling_from_per_token(usable, mb, per_token_f16);
    if desired <= f16_ceiling {
        // Fits at full precision — nothing to do, no message.
        return LaunchPlan { ctx: desired, flash_attn: false, kv: KvType::F16, note: None };
    }

    // Full-precision KV won't hold the desired window. Q8 halves the per-token cost
    // (kv_cache_bytes_at's exact integer divisor — bit-identical to the former `/ 2`).
    let per_token_q8 = kv_cache_bytes_at(KvPrecision::Q8, d.layers, d.head_count, d.head_count_kv, d.embedding_length, 1);
    let q8_ceiling = ceiling_from_per_token(usable, mb, per_token_q8);
    let ctx = desired.min(q8_ceiling).max(MIN_CONTEXT);
    let gb = total_bytes as f64 / 1_000_000_000.0;
    let note = Some(if ctx < desired {
        format!(
            "Detected {gb:.0} GB of RAM — not enough to hold a {desired}-token context for this model \
             at full precision. Running safely: enabled Flash Attention and a Q8 KV cache (half the \
             memory) and capped the context to {ctx} tokens so the GPU can't run out of memory."
        )
    } else {
        format!(
            "Detected {gb:.0} GB of RAM. Running safely: enabled Flash Attention and a Q8 KV cache \
             (half the memory) so the {ctx}-token context fits without a GPU out-of-memory error."
        )
    });
    LaunchPlan { ctx, flash_attn: true, kv: KvType::Q8, note }
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
    let mut cmd = Host::command(dir.join(bin_name()));
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in Host::envs_for_lib_dir(dir) {
        cmd.env(k, v);
    }
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

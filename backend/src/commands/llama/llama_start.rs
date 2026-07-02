use crate::commands::llama::llama_runtime::{
    bin_name, build_spawn_args, is_reachable, jinja_unsupported, plan_launch, spawn_meta, spawn_server,
    spawn_stderr_tail, wait_until_ready, SpawnMeta, JINJA_UNSUPPORTED_MSG, PORT, PROBE_TIMEOUT_MS,
};
use crate::commands::system::hardware::snapshot;
use crate::commands::llama::llama_server_types::{LlamaServerState, LlamaStartResult, SpawnReadout};
use crate::commands::llama::llama_templates::{model_stem, resolve_template_file};
use crate::errors::AppError;
use std::path::PathBuf;
use tauri::Manager;

pub const READY_TIMEOUT_MSG: &str =
    "llama-server started but didn't become reachable within 30 seconds.";
pub const NOT_BUNDLED_MSG: &str = "The llama-server sidecar isn't bundled for this platform yet.";

/// Directory holding `llama-server` and its dylibs. They must stay colocated
/// (`@loader_path` resolves the libs), so we resolve the whole dir, not a lone
/// binary: env override → bundled resources (prod) → source tree (dev).
fn llama_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUANTAMIND_LLAMA_DIR") {
        return has_bin(PathBuf::from(p));
    }
    if let Ok(res) = app.path().resource_dir() {
        if let Some(d) = has_bin(res.join("binaries")) {
            return Some(d);
        }
    }
    #[cfg(debug_assertions)]
    if let Ok(exe) = std::env::current_exe() {
        // target/debug/<app> → backend/binaries
        if let Some(dev) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            return has_bin(dev.join("binaries"));
        }
    }
    None
}

fn has_bin(dir: PathBuf) -> Option<PathBuf> {
    dir.join(bin_name()).exists().then_some(dir)
}

#[tauri::command]
pub async fn start_llama_server(
    app: tauri::AppHandle,
    state: tauri::State<'_, LlamaServerState>,
    model_path: String,
    num_ctx: Option<u32>,
) -> Result<LlamaStartResult, AppError> {
    // One GGUF read → context window + architecture + KV dims. The arch (and the
    // model name) resolve any user/bundled `.jinja` override for a model whose
    // embedded template is broken; no override ⇒ the embedded template via `--jinja`.
    // `num_ctx` (the user's "Context window" param) drives the launch `-c`, bounded by
    // the model max — llama.cpp can't change context per request, so it's set here.
    let SpawnMeta { ctx: gguf_ctx, arch, dims } = spawn_meta(&model_path);
    // The model's on-disk footprint (the dominant resident-memory term) — read once
    // for both the hardware ceiling and the spawn readout; `None` if it can't be
    // stat'd, never fabricated.
    let model_bytes = std::fs::metadata(&model_path).map(|m| m.len()).ok();
    // Bound `-c` to what this machine's RAM can hold (weights + KV cache) so even an
    // explicit high `num_ctx` can't OOM the pre-allocated cache. Budgeted on TOTAL
    // memory (a stable per-machine capacity), not momentary free RAM. If the weight
    // size is unknown we can't measure a budget → no clamp (u32::MAX), never a bogus
    // cap that would defeat an explicit window; the unset default still caps at 8K.
    // Hardware-aware launch plan: the safe `-c` plus, on a memory-tight host, flash attention
    // + a Q8 KV cache (so the requested context fits instead of OOM-wedging llama.cpp), plus a
    // user-facing note describing the constraint. On a roomy machine this is the old behaviour
    // (full-precision KV, no forced flags, no note).
    let plan = plan_launch(model_bytes, dims, snapshot().total_memory_bytes, gguf_ctx, num_ctx);
    let ctx = plan.ctx;
    // Already serving this exact (model, context)? No-op. A changed context falls
    // through and relaunches with the new `-c`.
    if is_reachable(PROBE_TIMEOUT_MS).await && state.is_current(&model_path, ctx) {
        return Ok(LlamaStartResult::AlreadyRunning);
    }
    state.stop().map_err(AppError::Internal)?;
    let Some(dir) = llama_dir(&app) else {
        return Ok(LlamaStartResult::NotBundled {
            note: NOT_BUNDLED_MSG.into(),
        });
    };
    let template = resolve_template_file(&app, model_stem(&model_path), &arch);
    let template_arg = template.as_deref().and_then(|p| p.to_str());
    // Stamp the load window right before exec: spawn → first `/health`-ready is the
    // model-load time (coarse, bounded by the 500ms poll), excluding our arg-prep.
    let load_start = std::time::Instant::now();
    let mut child = match spawn_server(&dir, &build_spawn_args(&model_path, PORT, &plan, template_arg)) {
        Ok(c) => c,
        Err(error) => return Ok(LlamaStartResult::StartFailed { error }),
    };
    let pid = child.id();
    // Drain stderr so a stale-binary death (e.g. `--jinja` rejected) leaves a
    // diagnosable tail, and so the pipe never fills and wedges the child.
    let tail = child.stderr.take().map(spawn_stderr_tail);
    state.store(child, model_path, ctx);
    if wait_until_ready().await {
        // Ready: record the one-time spawn readout (only on success → no bogus
        // load_ms for a failed/never-ready start).
        state.set_readout(SpawnReadout { model_bytes, load_ms: load_start.elapsed().as_millis() as u64 });
        Ok(LlamaStartResult::Started { pid, port: PORT, note: plan.note })
    } else {
        let _ = state.stop();
        let stale = tail
            .as_ref()
            .map(|t| jinja_unsupported(&t.lock().unwrap_or_else(|p| p.into_inner())))
            .unwrap_or(false);
        let error = if stale {
            JINJA_UNSUPPORTED_MSG.into()
        } else {
            READY_TIMEOUT_MSG.into()
        };
        Ok(LlamaStartResult::StartFailed { error })
    }
}

#[tauri::command]
pub async fn stop_llama_server(state: tauri::State<'_, LlamaServerState>) -> Result<(), AppError> {
    state.stop().map_err(AppError::Internal)
}

/// One-time spawn readout for the running llama-server (model footprint + load
/// time). `None` when no server is up — the frontend then shows nothing rather
/// than a fabricated phase. Mirrors `mlx_server_status`.
#[tauri::command]
pub fn llama_server_info(state: tauri::State<'_, LlamaServerState>) -> Option<SpawnReadout> {
    state.readout()
}

#[cfg(test)]
#[path = "llama_start_tests.rs"]
mod tests;

use crate::errors::AppError;
use serde::Serialize;

/// One currently-resident model. `size_vram_bytes` is the VRAM portion of the
/// `size_bytes` total footprint (the rest sits in system RAM); it stays 0 when
/// the split is unmeasured, never a fabricated GPU residency. `context_length`
/// is the launch window.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LoadedModel {
    pub name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
    pub context_length: Option<u32>,
}

/// Fold the running llama.cpp model into the loaded list. `size_bytes` is the GGUF's
/// on-disk footprint (the dominant resident term); `context_length` is the launch `-c`.
/// `size_vram_bytes` stays 0 — the GPU split is unmeasured here (the app sets no `-ngl`,
/// so llama.cpp defaults to CPU), never a fabricated GPU residency. The unified-memory
/// case (the whole model resident with no offload) is handled once, backend-agnostically,
/// in the frontend `vramUsage(unified)`.
fn append_llama(models: &mut Vec<LoadedModel>, running: Option<(String, u32)>) {
    let Some((path, ctx)) = running else { return };
    let name = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    // ctx 0 means "unknown" (an external server's /props didn't report it) — carry None, not a
    // fabricated 0-token window that the Context Budget bar would render as "N / 0 ctx".
    let context_length = (ctx > 0).then_some(ctx);
    models.push(LoadedModel { name, size_bytes, size_vram_bytes: 0, context_length });
}

#[tauri::command]
pub async fn get_loaded_models(
    llama: tauri::State<'_, crate::commands::llama::llama_server_types::LlamaServerState>,
) -> Result<Vec<LoadedModel>, AppError> {
    let mut models = Vec::new();
    // Prefer the app-spawned server (trusted state, no HTTP). When the app started nothing, probe
    // the llama.cpp port for an EXTERNALLY-started server (a manual `llama-server`, or one the `qm`
    // CLI launched) — otherwise its loaded model shows "Not available" in the Inspector despite
    // being resident. The probe is best-effort and only runs when there's no app-managed server.
    let running = match llama.running_summary() {
        Some(s) => Some(s),
        None => {
            use crate::commands::llama::llama_runtime::{probe_running_model, PROBE_TIMEOUT_MS};
            probe_running_model(PROBE_TIMEOUT_MS).await
        }
    };
    append_llama(&mut models, running);
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_llama_adds_running_model_by_stem() {
        let mut models = vec![];
        append_llama(&mut models, Some(("/Users/x/.quantamind/gguf/qwen2.5-coder-7b_q4_k_m.gguf".into(), 8192)));
        assert_eq!(models.len(), 1);
        // Name is the file STEM (matches the Inspector's llama.cpp row.model), ctx carried.
        assert_eq!(models[0].name, "qwen2.5-coder-7b_q4_k_m");
        assert_eq!(models[0].context_length, Some(8192));
        assert_eq!(models[0].size_vram_bytes, 0); // GPU split unmeasured; unified handled in the UI
    }

    #[test]
    fn append_llama_is_a_noop_when_no_server_runs() {
        let mut models = vec![];
        append_llama(&mut models, None);
        assert!(models.is_empty());
    }

    #[test]
    fn append_llama_unknown_ctx_is_none_not_a_zero_window() {
        // An external server whose /props reported no n_ctx (ctx 0) must not surface a
        // fabricated 0-token context window; the row still appears for its KV ceilings.
        let mut models = vec![];
        append_llama(&mut models, Some(("/Users/x/.quantamind/gguf/gpt-oss-20b_q4_k_m.gguf".into(), 0)));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "gpt-oss-20b_q4_k_m");
        assert_eq!(models[0].context_length, None);
    }
}

use crate::errors::{AppError, AppResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_OLLAMA: &str = "http://localhost:11434";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// One currently-loaded Ollama model from `/api/ps`. `size_vram` is the VRAM
/// portion of the `size` total footprint (the rest is offloaded to system RAM);
/// `/api/ps` omits it when 0 (100% CPU), so it defaults to 0. `context_length`
/// is a newer field — Ollama preallocates the full-context KV cache into VRAM.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LoadedModel {
    pub name: String,
    pub size_bytes: u64,
    pub size_vram_bytes: u64,
    pub context_length: Option<u32>,
}

#[derive(Deserialize)]
struct PsResponse {
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    size_vram: u64,
    #[serde(default)]
    context_length: Option<u32>,
}

pub async fn fetch_loaded(endpoint: &str, timeout: Duration) -> AppResult<Vec<LoadedModel>> {
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    // Degrade to empty (not an error) when Ollama is unreachable: the Inspector
    // then shows "not available" per row instead of failing the whole view.
    let Ok(resp) = client.get(format!("{endpoint}/api/ps")).send().await else {
        return Ok(vec![]);
    };
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let body: PsResponse = resp
        .json()
        .await
        .map_err(|e| AppError::Inference(format!("ps body: {e}")))?;
    Ok(body
        .models
        .into_iter()
        .map(|m| LoadedModel {
            name: m.name,
            size_bytes: m.size,
            size_vram_bytes: m.size_vram,
            context_length: m.context_length,
        })
        .collect())
}

/// Fold the running llama.cpp model into the loaded list: Ollama's `/api/ps` only knows
/// Ollama models, so a llama.cpp column would otherwise show "not loaded". `size_bytes` is the
/// GGUF's on-disk footprint (the dominant resident term); `context_length` is the launch `-c`.
///
/// `size_vram_bytes`: on **unified** memory (Apple Silicon) the CPU and GPU share ONE pool, so
/// the whole model is resident "in unified memory (VRAM)" — matching how Ollama's `/api/ps`
/// reports it — and there is no "offload to RAM". On a **discrete** GPU the app passes no
/// `-ngl`, so llama.cpp defaults to CPU (weights in system RAM, not VRAM); reporting `0` there
/// is honest (the bar then shows it as offloaded), never a fabricated GPU residency.
fn append_llama(models: &mut Vec<LoadedModel>, running: Option<(String, u32)>, unified: bool) {
    let Some((path, ctx)) = running else { return };
    let name = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let size_vram_bytes = if unified { size_bytes } else { 0 };
    models.push(LoadedModel { name, size_bytes, size_vram_bytes, context_length: Some(ctx) });
}

#[tauri::command]
pub async fn get_loaded_models(
    llama: tauri::State<'_, crate::commands::llama::llama_server_types::LlamaServerState>,
) -> Result<Vec<LoadedModel>, AppError> {
    let mut models = fetch_loaded(DEFAULT_OLLAMA, DEFAULT_TIMEOUT).await?;
    let unified = crate::commands::system::hardware::snapshot().gpu.unified;
    append_llama(&mut models, llama.running_summary(), unified);
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_llama_adds_running_model_by_stem() {
        let mut models = vec![];
        append_llama(&mut models, Some(("/Users/x/.quantamind/gguf/qwen2.5-coder-7b_q4_k_m.gguf".into(), 8192)), false);
        assert_eq!(models.len(), 1);
        // Name is the file STEM (matches the Inspector's llama.cpp row.model), ctx carried.
        assert_eq!(models[0].name, "qwen2.5-coder-7b_q4_k_m");
        assert_eq!(models[0].context_length, Some(8192));
    }

    #[test]
    fn unified_reports_model_resident_in_vram_discrete_reports_zero() {
        // Unified (Apple Silicon): one shared pool → the whole model is "in unified memory",
        // matching Ollama's /api/ps, so size_vram == size_bytes (no false "offloaded to RAM").
        let mut u = vec![];
        append_llama(&mut u, Some(("/x/model_q4.gguf".into(), 8192)), true);
        assert_eq!(u[0].size_vram_bytes, u[0].size_bytes);
        // Discrete + no -ngl → llama.cpp defaults to CPU (weights in RAM), so 0 is honest.
        let mut d = vec![];
        append_llama(&mut d, Some(("/x/model_q4.gguf".into(), 8192)), false);
        assert_eq!(d[0].size_vram_bytes, 0);
    }

    #[test]
    fn append_llama_is_a_noop_when_no_server_runs() {
        let mut models = vec![];
        append_llama(&mut models, None, true);
        assert!(models.is_empty());
    }
}

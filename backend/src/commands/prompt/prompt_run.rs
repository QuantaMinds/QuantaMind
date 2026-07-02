#![deny(clippy::unwrap_used)]

use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::llama::llama_backend::LlamaCppBackend;
use crate::inference::mlx::mlx_backend::MlxBackend;
use crate::inference::ollama::ollama::GenerateOptions;
use crate::inference::ollama::ollama_backend::OllamaBackend;
use tokio_util::sync::CancellationToken;

pub fn validate(model: &str, prompt: &str) -> AppResult<()> {
    if model.trim().is_empty() {
        return Err(AppError::Validation("model is empty".into()));
    }
    if prompt.trim().is_empty() {
        return Err(AppError::Validation("prompt is empty".into()));
    }
    Ok(())
}

pub async fn run_prompt_inner(
    backend: BackendKind,
    endpoint: &str,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    options: Option<GenerateOptions>,
    keep_alive: Option<i32>,
    cancel: CancellationToken,
    on_token: impl FnMut(&str),
) -> AppResult<GenerateStats> {
    validate(model, prompt)?;
    let spec = GenerateSpec {
        model: model.to_string(),
        prompt: prompt.to_string(),
        system: system.map(str::to_string),
        options,
        keep_alive,
        think: None,
    };
    match backend {
        BackendKind::Ollama => {
            OllamaBackend::new(endpoint.to_string()).generate(&spec, cancel, on_token).await
        }
        BackendKind::LlamaCpp => {
            LlamaCppBackend::new(endpoint.to_string()).generate(&spec, cancel, on_token).await
        }
        BackendKind::Mlx => {
            MlxBackend::new(endpoint.to_string(), model.to_string())
                .generate(&spec, cancel, on_token)
                .await
        }
    }
}

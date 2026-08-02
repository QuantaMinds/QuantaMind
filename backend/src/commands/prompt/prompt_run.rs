#![deny(clippy::unwrap_used)]

use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::remote_config;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::llama::llama_backend::LlamaCppBackend;
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::vllm::vllm_backend::VLlmBackend;
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
        BackendKind::LlamaCpp => {
            LlamaCppBackend::new(endpoint.to_string()).generate(&spec, cancel, on_token).await
        }
        BackendKind::VLlm => {
            VLlmBackend::new(endpoint.to_string(), remote_config::vllm().api_key, model.to_string())
                .generate(&spec, cancel, on_token)
                .await
        }
    }
}

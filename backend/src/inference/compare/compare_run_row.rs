#![deny(clippy::unwrap_used)]
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::backend::remote_config;
use crate::inference::llama::llama_backend::LlamaCppBackend;
use crate::inference::mlx::mlx_backend::MlxBackend;
use crate::inference::sglang::sglang_backend::SgLangBackend;
use crate::inference::vllm::vllm_backend::VLlmBackend;
use crate::inference::compare::compare_runner::RowSpec;
use crate::inference::compare::compare_runner_finalize::finalize_row;
use crate::inference::compare::compare_sink::CompareSink;
use crate::inference::compare::compare_state::CompareRunState;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::ollama::ollama_backend::OllamaBackend;
use crate::inference::token_handler::make_token_handler;
use crate::metrics::timing::RunTiming;
use crate::sync::MutexExt;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// The HTTP endpoint a compare row talks to. Ollama rows use the run's configured
/// endpoint; every other backend resolves via `endpoint::base_url` (llama.cpp's
/// sidecar default, MLX's dynamic port, or the user-configured vLLM/SGLang URL).
fn endpoint_for(ollama_endpoint: &str, backend: BackendKind) -> String {
    match backend {
        BackendKind::Ollama => ollama_endpoint.to_string(),
        _ => endpoint::base_url(backend),
    }
}

pub(crate) async fn run_one_row(
    sink: &Arc<dyn CompareSink>,
    state: &CompareRunState,
    endpoint: &str,
    row: &RowSpec,
    prompt: &str,
    system: Option<&str>,
    keep_alive: Option<i32>,
) {
    let row_token = CancellationToken::new();
    state.rows.lock_recover().insert(row.model_id, row_token.clone());
    let id_str = row.model_id.to_string();
    sink.loading(&id_str, &row.model);
    let timing = Arc::new(Mutex::new(RunTiming::start()));
    let sink_for_token = sink.clone();
    let id_for_token = id_str.clone();
    let model_for_token = row.model.clone();
    let handler = make_token_handler(
        move |t| {
            sink_for_token.token(&id_for_token, &model_for_token, t);
            Ok(())
        },
        row_token.clone(), timing.clone(),
    );
    let options = row.options.clone();
    let spec = GenerateSpec {
        model: row.model.clone(),
        prompt: prompt.to_string(),
        system: system.map(str::to_string),
        options,
        keep_alive,
        think: None,
    };
    let row_endpoint = endpoint_for(endpoint, row.backend);
    let result = match row.backend {
        BackendKind::Ollama => {
            OllamaBackend::new(row_endpoint)
                .generate(&spec, row_token.clone(), handler)
                .await
        }
        BackendKind::LlamaCpp => {
            LlamaCppBackend::new(row_endpoint)
                .generate(&spec, row_token.clone(), handler)
                .await
        }
        BackendKind::Mlx => {
            MlxBackend::new(row_endpoint, row.model.clone())
                .generate(&spec, row_token.clone(), handler)
                .await
        }
        BackendKind::VLlm => {
            VLlmBackend::new(row_endpoint, remote_config::vllm().api_key, row.model.clone())
                .generate(&spec, row_token.clone(), handler)
                .await
        }
        BackendKind::SgLang => {
            SgLangBackend::new(row_endpoint, remote_config::sglang().api_key, row.model.clone())
                .generate(&spec, row_token.clone(), handler)
                .await
        }
    };
    state.rows.lock_recover().remove(&row.model_id);
    finalize_row(sink.as_ref(), row, &timing, &row_token, result);
}

#[cfg(test)]
#[path = "compare_run_row_tests.rs"]
mod tests;

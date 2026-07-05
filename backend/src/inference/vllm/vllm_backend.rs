use crate::errors::AppResult;
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::openai::chat_stream::stream_generate;
use tokio_util::sync::CancellationToken;

/// Streams generations from a remote vLLM `/v1/chat/completions` endpoint
/// (OpenAI-compatible SSE). Like MLX it is multi-model, so `spec.model` IS sent;
/// unlike the local sidecars it runs on a remote GPU, so it carries the
/// user-configured `endpoint` + optional `api_key` (bearer). `spec.keep_alive`
/// has no vLLM equivalent and is not part of the request.
pub struct VLlmBackend {
    endpoint: String,
    api_key: Option<String>,
    model: String,
}

impl VLlmBackend {
    pub fn new(endpoint: String, api_key: Option<String>, model: String) -> Self {
        Self { endpoint, api_key, model }
    }
}

impl InferenceBackend for VLlmBackend {
    async fn generate<F: FnMut(&str)>(
        &self,
        spec: &GenerateSpec,
        cancel: CancellationToken,
        on_token: F,
    ) -> AppResult<GenerateStats> {
        stream_generate(
            &self.endpoint,
            self.api_key.as_deref(),
            &self.model,
            &spec.prompt,
            spec.system.as_deref(),
            spec.options.clone(),
            spec.think,
            cancel,
            on_token,
        )
        .await
    }
}

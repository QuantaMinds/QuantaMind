use crate::errors::AppResult;
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::openai::chat_stream::stream_generate;
use tokio_util::sync::CancellationToken;

/// Streams generations from an `mlx_lm.server` `/v1/chat/completions` endpoint.
/// Unlike `llama-server`, mlx_lm.server is multi-model, so `spec.model` IS sent.
/// `spec.keep_alive` has no mlx_lm equivalent and is not part of the request.
pub struct MlxBackend {
    endpoint: String,
    model: String,
}

impl MlxBackend {
    pub fn new(endpoint: String, model: String) -> Self {
        Self { endpoint, model }
    }
}

impl InferenceBackend for MlxBackend {
    async fn generate<F: FnMut(&str)>(
        &self,
        spec: &GenerateSpec,
        cancel: CancellationToken,
        on_token: F,
    ) -> AppResult<GenerateStats> {
        stream_generate(
            &self.endpoint,
            None, // mlx_lm.server is local and unauthenticated
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

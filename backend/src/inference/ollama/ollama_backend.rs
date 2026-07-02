use crate::errors::AppResult;
use crate::inference::backend::backend::InferenceBackend;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::ollama::ollama::stream_generate;
use tokio_util::sync::CancellationToken;

/// Streams generations from an Ollama HTTP server's `/api/generate`.
pub struct OllamaBackend {
    endpoint: String,
}

impl OllamaBackend {
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }
}

impl InferenceBackend for OllamaBackend {
    async fn generate<F: FnMut(&str)>(
        &self,
        spec: &GenerateSpec,
        cancel: CancellationToken,
        on_token: F,
    ) -> AppResult<GenerateStats> {
        stream_generate(
            &self.endpoint,
            &spec.model,
            &spec.prompt,
            spec.system.as_deref(),
            spec.options.clone(),
            spec.keep_alive,
            spec.think,
            cancel,
            on_token,
        )
        .await
    }
}

use crate::errors::AppResult;
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use tokio_util::sync::CancellationToken;

/// One streaming generation against some backend. Implementors stream
/// response text through `on_token` and return the server-reported final
/// metrics (`GenerateStats`) when the model is done, or default stats if
/// `cancel` fires. The same contract for llama.cpp, and cloud, so
/// callers stay backend-agnostic and select via a `BackendKind` match.
#[allow(async_fn_in_trait)]
pub trait InferenceBackend {
    async fn generate<F: FnMut(&str)>(
        &self,
        spec: &GenerateSpec,
        cancel: CancellationToken,
        on_token: F,
    ) -> AppResult<GenerateStats>;
}

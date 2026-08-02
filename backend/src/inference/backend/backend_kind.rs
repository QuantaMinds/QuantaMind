use serde::{Deserialize, Serialize};

/// Which backend serves a model. llama.cpp (a bundled `llama-server` sidecar)
/// runs locally; vLLM is a remote OpenAI-compatible GPU server (its endpoint is
/// user-configured — see `remote_config`). Surfaces as `ModelInfo.backend` over
/// IPC and selects the backend in eval dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    #[default]
    LlamaCpp,
    /// Remote vLLM server (OpenAI `/v1/chat/completions`). Wire string `"vllm"`.
    #[serde(rename = "vllm")]
    VLlm,
}

#[cfg(test)]
#[path = "backend_kind_tests.rs"]
mod tests;

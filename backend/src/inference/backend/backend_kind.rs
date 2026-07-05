use serde::{Deserialize, Serialize};

/// Which backend serves a model. Ollama and llama.cpp (a bundled `llama-server`
/// sidecar) and `mlx_lm.server` on Apple Silicon run locally; vLLM and SGLang are
/// remote OpenAI-compatible GPU servers (their endpoint is user-configured — see
/// `remote_config`). Surfaces as `ModelInfo.backend` over IPC and selects the
/// backend in compare/eval dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    #[default]
    Ollama,
    LlamaCpp,
    Mlx,
    /// Remote vLLM server (OpenAI `/v1/chat/completions`). Wire string `"vllm"`.
    #[serde(rename = "vllm")]
    VLlm,
    /// Remote SGLang server (OpenAI `/v1/chat/completions`). Wire string `"sglang"`.
    #[serde(rename = "sglang")]
    SgLang,
}

#[cfg(test)]
#[path = "backend_kind_tests.rs"]
mod tests;

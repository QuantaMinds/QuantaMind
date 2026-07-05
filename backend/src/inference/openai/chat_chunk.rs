use serde::Deserialize;

/// One streamed `data:` chunk from an OpenAI-compatible SSE endpoint
/// (mlx_lm.server, vLLM, SGLang). `usage` may be absent mid-stream (and is
/// version-dependent even on the terminal chunk), so it is optional and the stats
/// layer leaves counts `None` when it never arrives — never fabricate.
#[derive(Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub delta: Delta,
    /// `"stop"` or `"length"`; presence (not value) marks the terminal chunk.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    /// A reasoning model's thinking stream, delivered in a separate field (NOT `content`). The field
    /// name differs by server: mlx_lm.server (≥0.31) uses `reasoning`; vLLM/SGLang reasoning parsers
    /// use `reasoning_content` — accept both via alias. (Ollama uses `thinking` on its native `/api`
    /// path, handled separately.) When a `has_thinking` model reasons, this carries the scratchpad
    /// and `content` holds only the answer. Captured and re-wrapped as inline `<think>…</think>` so
    /// `strip_think` + D9 accounting handle every backend identically.
    #[serde(default, alias = "reasoning_content")]
    pub reasoning: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Strip an SSE `data: ` prefix if present; a bare-JSON line is accepted too.
pub fn strip_sse(line: &[u8]) -> &[u8] {
    line.strip_prefix(b"data: ").unwrap_or(line)
}

#[cfg(test)]
#[path = "chat_chunk_tests.rs"]
mod tests;

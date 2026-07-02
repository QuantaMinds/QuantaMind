use serde::Deserialize;

/// One streamed `data:` chunk from mlx_lm.server's OpenAI-compatible SSE.
/// `usage` may be absent mid-stream (and is version-dependent even on the
/// terminal chunk), so it is optional and the stats layer leaves counts `None`
/// when it never arrives — never fabricate.
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
    /// A reasoning model's thinking stream. mlx_lm.server (≥0.31) puts it in a `reasoning` field
    /// (NOT `content`, NOT llama.cpp's `reasoning_content`, NOT Ollama's `thinking` — a THIRD name;
    /// verified live + against ml-explore/mlx-lm#1352). When a `has_thinking` model reasons, this
    /// carries the scratchpad and `content` holds only the answer. Captured and re-wrapped as inline
    /// `<think>…</think>` so `strip_think` + D9 accounting handle MLX like the other backends.
    #[serde(default)]
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
#[path = "mlx_chunk_tests.rs"]
mod tests;

use crate::inference::generate::generate_options::GenerateOptions;
use serde::Serialize;

/// An OpenAI-compatible `/v1/chat/completions` request, shared by every backend
/// that speaks that wire (`mlx_lm.server`, vLLM, SGLang). Each server is
/// multi-model, so `model` is sent. System text becomes a `system` message — the
/// endpoint applies the chat template. `top_k`/`repetition_penalty` are accepted
/// as extensions by all three servers; there is **no `seed` field** (mlx_lm.server
/// has none, so a seed is intentionally dropped and these runs are not
/// seed-reproducible).
#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    /// Ask for a final `usage` chunk on the SSE stream. vLLM/SGLang omit `usage`
    /// from streamed responses unless this is set (verified live — token counts
    /// came back `None` without it); mlx_lm.server sends usage regardless and
    /// tolerates the flag. Required or the Latency tab's token counts read "Not
    /// available" for the remote backends.
    pub stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f32>,
    /// Controls a reasoning model's thinking. The server forwards `chat_template_kwargs` into the
    /// jinja template; Qwen3/`has_thinking` models read `enable_thinking`. Sent explicitly (both
    /// true and false) so an `is_thinking=false` eval run is TRULY non-thinking — otherwise a
    /// `has_thinking` model reasons by default and blows the smaller non-thinking budget. Ignored by
    /// non-reasoning templates (jinja drops unknown kwargs). Supported by mlx_lm.server, vLLM, and
    /// SGLang (verified vs ml-explore/mlx-lm#1352).
    pub chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Serialize)]
pub struct ChatTemplateKwargs {
    pub enable_thinking: bool,
}

#[derive(Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

#[derive(Serialize)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

impl ChatRequest {
    pub fn new(
        model: String,
        prompt: String,
        system: Option<&str>,
        opts: Option<GenerateOptions>,
        think: Option<bool>,
    ) -> Self {
        let o = opts.unwrap_or_default();
        let mut messages = Vec::new();
        if let Some(s) = system.filter(|s| !s.is_empty()) {
            messages.push(Message { role: "system", content: s.to_string() });
        }
        messages.push(Message { role: "user", content: prompt });
        Self {
            model,
            messages,
            stream: true,
            stream_options: StreamOptions { include_usage: true },
            max_tokens: o.num_predict,
            temperature: o.temperature,
            top_p: o.top_p,
            top_k: o.top_k,
            repetition_penalty: o.repeat_penalty,
            chat_template_kwargs: ChatTemplateKwargs { enable_thinking: matches!(think, Some(true)) },
        }
    }
}

#[cfg(test)]
#[path = "chat_request_tests.rs"]
mod tests;

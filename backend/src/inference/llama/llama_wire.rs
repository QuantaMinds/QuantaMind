use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::llama::llama_timings::Timings;
use serde::{Deserialize, Serialize};

/// llama-server `/completion` request. Field names follow llama.cpp's server
/// (`n_predict`, not the server's `num_predict`); the model is fixed at spawn so the
/// body carries no model name. System text is prepended to the prompt —
/// `/completion` applies no chat template.
#[derive(Serialize)]
pub struct CompletionRequest {
    pub prompt: String,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_predict: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
}

impl CompletionRequest {
    pub fn new(prompt: String, opts: Option<GenerateOptions>) -> Self {
        let o = opts.unwrap_or_default();
        Self {
            prompt,
            stream: true,
            temperature: o.temperature,
            top_p: o.top_p,
            top_k: o.top_k,
            n_predict: o.num_predict,
            repeat_penalty: o.repeat_penalty,
            seed: o.seed,
        }
    }
}

#[derive(Deserialize)]
pub struct CompletionChunk {
    #[serde(default)]
    pub content: String,
    pub stop: bool,
    #[serde(default)]
    pub timings: Option<Timings>,
}

/// One streamed `/v1/chat/completions` chunk (OpenAI SSE). llama-server adds a
/// `timings` extension on the final chunk — the SAME prompt/predict ms the
/// `/completion` path reports — so per-phase stats (and the Inspector's TTFT
/// breakdown) survive the chat endpoint instead of collapsing to token-counts.
#[derive(Deserialize, Default)]
pub struct ChatStreamChunk {
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub timings: Option<Timings>,
}

#[derive(Deserialize, Default)]
pub struct ChatChoice {
    #[serde(default)]
    pub delta: ChatDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ChatDelta {
    #[serde(default)]
    pub content: Option<String>,
    /// A reasoning model's separate thinking stream. Modern llama-server (`--reasoning-format`
    /// default) EXTRACTS the `<think>` block out of `content` into this field; only the final
    /// answer stays in `content`. Captured and re-wrapped as inline `<think>…</think>` (mirroring
    /// the the server `thinking` field) so the runner's `strip_think` + the D9 accounting see reasoning
    /// on llama.cpp identically to the server. `None`/absent for a terse model or `--reasoning-format none`
    /// (which leaves `<think>` inline in `content`, still handled by `strip_think`).
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// llama-server `/v1/chat/completions` request (OpenAI-compatible). This is the
/// PRIMARY path: with `--jinja` at spawn the server applies the GGUF's embedded
/// chat template, giving the model its trained turn structure so it emits EOS
/// and stops — the `/completion` path (raw prompt, no template) is the fallback.
///
/// Unlike the shared OpenAI `ChatRequest`, this keeps `seed` (llama.cpp eval runs are
/// seed-reproducible and must stay so) and carries `stop` when set. The server
/// is single-model, so `model` is sent only for OpenAI-client compatibility.
#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

impl ChatRequest {
    pub fn new(
        model: String,
        prompt: String,
        system: Option<&str>,
        opts: Option<GenerateOptions>,
    ) -> Self {
        let o = opts.unwrap_or_default();
        let mut messages = Vec::new();
        if let Some(s) = system.filter(|s| !s.is_empty()) {
            messages.push(ChatMessage {
                role: "system",
                content: s.to_string(),
            });
        }
        messages.push(ChatMessage {
            role: "user",
            content: prompt,
        });
        Self {
            model,
            messages,
            stream: true,
            max_tokens: o.num_predict,
            temperature: o.temperature,
            top_p: o.top_p,
            top_k: o.top_k,
            repeat_penalty: o.repeat_penalty,
            seed: o.seed,
            stop: o.stop,
        }
    }
}

/// Strip an SSE `data: ` prefix if present. llama-server streams `/completion`
/// as `data: {json}` lines; a bare-JSON line is accepted too.
pub fn strip_sse(line: &[u8]) -> &[u8] {
    line.strip_prefix(b"data: ").unwrap_or(line)
}

#[derive(Deserialize)]
struct LlamaErrorBody {
    error: LlamaErrorInner,
}

#[derive(Deserialize)]
struct LlamaErrorInner {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    n_prompt_tokens: Option<u32>,
    #[serde(default)]
    n_ctx: Option<u32>,
}

/// Turn a llama-server error body into actionable copy when it's a context
/// overflow (`exceed_context_size_error`), else `None` (caller falls back to the
/// raw status+body). The window is fixed at launch (`-c`), so the cure is to
/// raise the "Context window" param and restart llama.cpp — not retry. Pure, so
/// the user-facing wording is tested without a live server.
pub fn context_overflow_hint(body: &str) -> Option<String> {
    let parsed: LlamaErrorBody = serde_json::from_str(body).ok()?;
    let e = parsed.error;
    let is_overflow = e.kind == "exceed_context_size_error"
        || e.message.contains("context size")
        || e.message.contains("exceeds the available context");
    if !is_overflow {
        return None;
    }
    let prompt = e.n_prompt_tokens.map_or("The prompt".into(), |n| format!("The prompt ({n} tokens)"));
    let window = e.n_ctx.map_or("the context window".into(), |n| format!("the {n}-token context window"));
    Some(format!(
        "{prompt} is larger than {window} this model was loaded with. Increase \
         \"Context window\" in the parameters, then restart llama.cpp (Stop & Start) — \
         its context is fixed at launch. Or shorten the prompt / reduce the Context \
         Stress Test length."
    ))
}

/// Turn a llama-server `500 Compute error` body into actionable copy. This is the
/// Metal/GPU compute failure — almost always the GPU (unified memory) running out
/// of room for the KV cache + compute buffer (`kIOGPUCommandBufferCallbackErrorOutOfMemory`
/// in the server's stderr, which the client never sees). CRITICAL: once it fires,
/// llama.cpp's backend is left "in an error state" and EVERY later request 500s the
/// same way until the server is restarted — so a silent generic error makes the whole
/// eval look like a model failure. The cure names the wedge AND the knobs (restart +
/// shrink the memory footprint). Pure, so the wording is tested without a live server.
pub fn compute_error_hint(body: &str) -> Option<String> {
    let parsed: LlamaErrorBody = serde_json::from_str(body).ok()?;
    let m = &parsed.error.message;
    let is_compute = m.contains("Compute error")
        || m.contains("failed to decode")
        || m.contains("OutOfMemory")
        || m.contains("out of memory");
    if !is_compute {
        return None;
    }
    Some(
        "llama.cpp hit a GPU compute error — the Mac's GPU (unified memory) ran out of room \
         for the model's context (KV cache). The server is now wedged and will fail every \
         request until you restart it: Stop & Start llama.cpp. To stop it recurring, lower \
         \"Context window\", load a smaller model or a lighter quant (e.g. Q4 instead of Q8), \
         or enable Flash Attention with a quantized KV cache."
            .to_string(),
    )
}

/// The single entry point the streaming paths use to rewrite a non-success llama-server
/// body: context-overflow copy first, then compute-error copy, else `None` (caller keeps
/// the raw status+body). Ordered so the more specific overflow message wins when both
/// could match.
pub fn llama_error_hint(body: &str) -> Option<String> {
    context_overflow_hint(body).or_else(|| compute_error_hint(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_num_predict_to_n_predict() {
        let opts = GenerateOptions {
            num_predict: Some(16),
            temperature: Some(0.2),
            ..Default::default()
        };
        let json = serde_json::to_string(&CompletionRequest::new("hi".into(), Some(opts))).unwrap();
        assert!(json.contains("\"n_predict\":16"));
        assert!(json.contains("\"temperature\":0.2"));
        assert!(json.contains("\"stream\":true"));
        assert!(!json.contains("num_predict"));
    }

    #[test]
    fn omits_unset_options() {
        let json = serde_json::to_string(&CompletionRequest::new("hi".into(), None)).unwrap();
        assert!(!json.contains("temperature"));
        assert!(!json.contains("seed"));
    }

    /// The exact body llama-server returns on the 400 → actionable copy naming
    /// the token counts and the cure (raise Context window + restart), NOT the
    /// raw JSON.
    #[test]
    fn context_overflow_hint_rewrites_the_400_body() {
        let body = r#"{"error":{"code":400,"message":"request (10536 tokens) exceeds the available context size (8192 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":10536,"n_ctx":8192}}"#;
        let msg = context_overflow_hint(body).expect("overflow body should produce a hint");
        assert!(msg.contains("10536 tokens"), "names the prompt size: {msg}");
        assert!(msg.contains("8192-token"), "names the loaded window: {msg}");
        assert!(msg.contains("Context window"), "tells the user which param to raise: {msg}");
        assert!(msg.contains("restart"), "tells the user to restart: {msg}");
    }

    /// A non-overflow error body is left for the caller's raw fallback.
    #[test]
    fn context_overflow_hint_ignores_unrelated_errors() {
        assert!(context_overflow_hint(r#"{"error":{"type":"server_error","message":"boom"}}"#).is_none());
        assert!(context_overflow_hint("boom").is_none(), "non-JSON body → None");
    }

    /// The exact `500 Compute error` body a Metal-OOM wedge returns → actionable copy that
    /// names the wedge (restart) AND the memory knobs (Context window / smaller model / quant),
    /// NOT the raw `{"message":"Compute error."}` that reads as a model failure.
    #[test]
    fn compute_error_hint_rewrites_the_500_body() {
        let body = r#"{"error":{"code":500,"message":"Compute error.","type":"server_error"}}"#;
        let msg = compute_error_hint(body).expect("compute-error body should produce a hint");
        assert!(msg.contains("GPU"), "names the GPU/memory cause: {msg}");
        assert!(msg.to_lowercase().contains("restart"), "tells the user to restart the wedged server: {msg}");
        assert!(msg.contains("Context window"), "names a knob to shrink the footprint: {msg}");
    }

    /// `failed to decode` (the other shape the same Metal-OOM wedge surfaces) also maps.
    #[test]
    fn compute_error_hint_matches_failed_to_decode() {
        let body = r#"{"error":{"code":500,"message":"failed to decode, ret = -3","type":"server_error"}}"#;
        assert!(compute_error_hint(body).is_some(), "failed-to-decode is the same wedge");
    }

    /// The combined entry point prefers the specific overflow copy, falls through to compute,
    /// and leaves a genuinely unrelated error for the caller's raw fallback.
    #[test]
    fn llama_error_hint_routes_overflow_then_compute_then_none() {
        let overflow = r#"{"error":{"code":400,"message":"exceeds the available context size","type":"exceed_context_size_error","n_prompt_tokens":9,"n_ctx":8}}"#;
        assert!(llama_error_hint(overflow).unwrap().contains("Context window"));
        let compute = r#"{"error":{"code":500,"message":"Compute error.","type":"server_error"}}"#;
        assert!(llama_error_hint(compute).unwrap().contains("GPU"));
        assert!(llama_error_hint(r#"{"error":{"type":"server_error","message":"boom"}}"#).is_none());
    }

    #[test]
    fn strip_sse_removes_data_prefix_only_when_present() {
        assert_eq!(strip_sse(b"data: {\"x\":1}"), b"{\"x\":1}");
        assert_eq!(strip_sse(b"{\"x\":1}"), b"{\"x\":1}");
    }

    #[test]
    fn chat_request_splits_system_and_user_messages() {
        let json = serde_json::to_string(&ChatRequest::new(
            "m".into(),
            "hi".into(),
            Some("be brief"),
            None,
        ))
        .unwrap();
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"content\":\"be brief\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hi\""));
    }

    /// The reason for a llama-specific request: seed-reproducibility and stops
    /// must survive onto the chat wire (the shared OpenAI ChatRequest drops seed).
    #[test]
    fn chat_request_preserves_seed_and_stop() {
        let opts = GenerateOptions {
            seed: Some(42),
            stop: Some(vec!["<|im_end|>".into()]),
            num_predict: Some(128),
            ..Default::default()
        };
        let json =
            serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, Some(opts)))
                .unwrap();
        assert!(
            json.contains("\"seed\":42"),
            "seed must reach the wire: {json}"
        );
        assert!(
            json.contains("<|im_end|>"),
            "stop must reach the wire: {json}"
        );
        assert!(json.contains("\"max_tokens\":128"));
    }

    #[test]
    fn chat_request_omits_unset_system_and_options() {
        let json =
            serde_json::to_string(&ChatRequest::new("m".into(), "hi".into(), None, None)).unwrap();
        assert!(!json.contains("system"));
        assert!(!json.contains("seed"));
        assert!(!json.contains("stop"));
    }
}

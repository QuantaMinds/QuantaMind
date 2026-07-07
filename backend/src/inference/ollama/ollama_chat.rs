use crate::errors::{AppError, AppResult};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::{ns_to_ms, GenerateStats};
use crate::inference::http::http::{body_or_note, streaming_client};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Ollama `/api/chat` request with native `tools`. `tools` is a pre-built JSON
/// array (the eval layer shapes it from its `ToolSchema`s) so this client stays
/// free of eval types — a backend client must not depend on the eval layer.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    tools: &'a Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerateOptions>,
    /// `Some(false)` disables a thinking-BY-DEFAULT model's hidden scratchpad (which would
    /// otherwise burn the turn budget before any tool call is emitted); `None` omits the field
    /// (the model's default). Accepted by every Ollama since the field existed — the server's
    /// capability check fires only on `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    message: ChatResponseMessage,
    /// `"stop"` vs `"length"` (num_predict cap → truncated); read by the agentic
    /// runner to retry a truncated turn rather than score it as a real failure.
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    load_duration: Option<u64>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    prompt_eval_duration: Option<u64>,
    #[serde(default)]
    eval_count: Option<u32>,
    #[serde(default)]
    eval_duration: Option<u64>,
    #[serde(default)]
    total_duration: Option<u64>,
}

#[derive(Deserialize, Default)]
struct ChatResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<ResponseToolCall>,
}

#[derive(Deserialize)]
struct ResponseToolCall {
    function: ResponseToolFn,
}

#[derive(Deserialize)]
struct ResponseToolFn {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// One native tool call, neutral of eval types: a name + a real argument object.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeToolCall {
    pub name: String,
    pub args: Value,
}

/// The translated `/api/chat` result: the real `tool_calls`, the assistant
/// `content` (for the caller's abstain check), and token stats.
pub struct ChatResult {
    pub tool_calls: Vec<NativeToolCall>,
    pub content: String,
    pub stats: GenerateStats,
}

impl ChatResponse {
    fn stats(&self) -> GenerateStats {
        GenerateStats {
            prompt_eval_count: self.prompt_eval_count,
            prompt_eval_ms: self.prompt_eval_duration.map(ns_to_ms),
            eval_count: self.eval_count,
            eval_ms: self.eval_duration.map(ns_to_ms),
            load_ms: self.load_duration.map(ns_to_ms),
            total_ms: self.total_duration.map(ns_to_ms),
            cache_n: None, // Ollama's /api/chat reports no prompt-cache count
            finish_reason: self.done_reason.clone(), // "stop" | "length"
        }
    }
}

/// Normalize a tool-call `arguments` value: some models return it as a JSON
/// *string* rather than an object — parse it back so the canonical args are a
/// real object (checkpoint/arg matching compares objects, not quoted strings).
/// Shared by every native backend (Ollama `/api/chat` and llama.cpp's OpenAI
/// `/v1/chat/completions`, whose builds disagree on string-vs-object args).
pub fn normalize_args(v: Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    }
}

/// Parse an `/api/chat` response body into the translated result. Split out so
/// the structured-output mapping is unit-tested without a live server.
pub(crate) fn parse_chat(json: &str) -> AppResult<ChatResult> {
    let parsed: ChatResponse =
        serde_json::from_str(json).map_err(|e| AppError::Inference(format!("bad chat response: {e}")))?;
    let stats = parsed.stats();
    let tool_calls = parsed
        .message
        .tool_calls
        .into_iter()
        .map(|tc| NativeToolCall { name: tc.function.name, args: normalize_args(tc.function.arguments) })
        .collect();
    Ok(ChatResult { tool_calls, content: parsed.message.content, stats })
}

/// Call Ollama's native `/api/chat` with a `tools` array and return the real
/// `tool_calls` (translation to canonical text + abstain handling live in the
/// caller). Non-streaming — tool responses are small.
pub async fn chat_with_tools(
    endpoint: &str,
    model: &str,
    system: &str,
    user: &str,
    tools: &Value,
    options: Option<GenerateOptions>,
    think: Option<bool>,
) -> AppResult<ChatResult> {
    let client = streaming_client()?;
    let messages = vec![
        ChatMessage { role: "system", content: system },
        ChatMessage { role: "user", content: user },
    ];
    let body =
        ChatRequest { model, messages, tools, options: options.filter(|o| !o.is_empty()), think, stream: false };
    let resp = client
        .post(format!("{endpoint}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                AppError::Timeout(format!("connect to Ollama: {e}"))
            } else {
                AppError::Inference(e.to_string())
            }
        })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Inference(format!("chat HTTP {status}: {}", body_or_note(resp).await)));
    }
    let text = resp.text().await.map_err(|e| AppError::Inference(e.to_string()))?;
    parse_chat(&text)
}

#[cfg(test)]
#[path = "ollama_chat_tests.rs"]
mod tests;

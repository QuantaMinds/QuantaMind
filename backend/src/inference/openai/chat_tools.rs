use crate::errors::{AppError, AppResult};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::http::http::{body_or_note, streaming_client};
use crate::inference::ollama::ollama_chat::{normalize_args, ChatResult, NativeToolCall};
use crate::inference::openai::chat_chunk::Usage;
use crate::inference::openai::chat_stats::from_usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A native tool-calling turn over an OpenAI-compatible `/v1/chat/completions`
/// endpoint with a `tools` array (non-streaming — tool responses are small). Used
/// by the remote vLLM/SGLang backends; when `api_key` is `Some`, an
/// `Authorization: Bearer` header is attached. Returns the SAME `ChatResult` as
/// Ollama/llama.cpp so the eval runner canonicalizes tool calls identically across
/// backends. (llama.cpp keeps its own client because it reads a llama-specific
/// `timings` extension; these servers report only `usage`.)
#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ToolChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    tools: &'a Value,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
struct Choice {
    #[serde(default)]
    message: ResponseMessage,
}

#[derive(Deserialize, Default)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
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
    /// OpenAI spec says this is a JSON *string*; some servers emit an object.
    /// `normalize_args` accepts either.
    #[serde(default)]
    arguments: Value,
}

/// Parse a `/v1/chat/completions` response body into the shared `ChatResult`.
/// Split out so the mapping is unit-tested without a live server.
pub(crate) fn parse_chat(json: &str) -> AppResult<ChatResult> {
    let parsed: ChatResponse = serde_json::from_str(json)
        .map_err(|e| AppError::Inference(format!("bad chat response: {e}")))?;
    let stats: GenerateStats = from_usage(parsed.usage);
    let msg = parsed.choices.into_iter().next().map(|c| c.message).unwrap_or_default();
    let tool_calls = msg
        .tool_calls
        .into_iter()
        .map(|tc| NativeToolCall { name: tc.function.name, args: normalize_args(tc.function.arguments) })
        .collect();
    Ok(ChatResult { tool_calls, content: msg.content.unwrap_or_default(), stats })
}

#[allow(clippy::too_many_arguments)]
pub async fn chat_with_tools(
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    system: &str,
    user: &str,
    tools: &Value,
    options: Option<GenerateOptions>,
) -> AppResult<ChatResult> {
    let client = streaming_client()?;
    let o = options.filter(|o| !o.is_empty()).unwrap_or_default();
    let body = ToolChatRequest {
        model,
        messages: vec![
            ChatMessage { role: "system", content: system },
            ChatMessage { role: "user", content: user },
        ],
        tools,
        stream: false,
        max_tokens: o.num_predict,
        temperature: o.temperature,
        top_p: o.top_p,
        top_k: o.top_k,
        seed: o.seed,
    };
    let mut req = client.post(format!("{endpoint}/v1/chat/completions")).json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() || e.is_connect() {
            AppError::Timeout(format!("connect to chat endpoint: {e}"))
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
#[path = "chat_tools_tests.rs"]
mod tests;

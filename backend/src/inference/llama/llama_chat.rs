use crate::errors::{AppError, AppResult};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::http::http::{body_or_note, streaming_client};
use crate::inference::llama::llama_timings::Timings;
use crate::inference::openai::chat_chunk::Usage;
use crate::inference::openai::chat_stats::from_usage;
use crate::inference::chat::native_call::{normalize_args, ChatResult, NativeToolCall};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// llama-server's OpenAI-compatible `/v1/chat/completions` with a native `tools`
/// array. Requires the server to be launched with `--jinja` (see
/// `commands/llama/llama_runtime::build_spawn_args`) so the embedded template's
/// tool grammar is applied. Non-streaming — tool responses are small.
///
/// Returns the SAME `ChatResult` as the server's `chat_with_tools` so the native
/// turn canonicalizes tool calls identically across backends. `tools` is a
/// pre-built JSON array (the eval layer shapes it) — a backend client must not
/// depend on the eval layer.
#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
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
    /// llama-server's per-phase ms extension — preferred over `usage` (which is
    /// token counts only) so prefill/predict ms reach `GenerateStats`.
    #[serde(default)]
    timings: Option<Timings>,
}

#[derive(Deserialize, Default)]
struct Choice {
    #[serde(default)]
    message: ResponseMessage,
    /// Why generation stopped — `"stop"` (natural end) vs `"length"` (hit the output cap →
    /// TRUNCATED). It lives on the CHOICE, not in `timings`/`usage`, so nothing downstream
    /// can recover it: this struct is the only place it exists on the wire. Omitting it made
    /// `stats.finish_reason` permanently `None` on every llama.cpp native turn, which turned
    /// the runner's truncation retry and its setting-vs-hardware split into dead code and
    /// laundered every truncation into `Malformed`/`Hallucinated`/`EmptyOutput`.
    #[serde(default)]
    finish_reason: Option<String>,
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
    /// OpenAI spec says this is a JSON *string*; some llama.cpp builds emit an
    /// object. `normalize_args` accepts either.
    #[serde(default)]
    arguments: Value,
}

/// Parse a `/v1/chat/completions` response body into the shared `ChatResult`.
/// Split out so the structured-output mapping is unit-tested without a server.
pub(crate) fn parse_chat(json: &str) -> AppResult<ChatResult> {
    let parsed: ChatResponse = serde_json::from_str(json)
        .map_err(|e| AppError::Inference(format!("bad chat response: {e}")))?;
    // The choice carries BOTH the message and the stop reason; take it once and keep both.
    // (Reading only `.message` here is what dropped `finish_reason` on the floor.)
    let choice = parsed.choices.into_iter().next().unwrap_or_default();
    let finish_reason = choice.finish_reason;
    // Prefer llama-server's `timings` (prompt/predict ms); fall back to token-count
    // `usage` when absent. Never fabricate a missing duration. `finish_reason` is threaded
    // into BOTH arms — it comes from the choice, so neither stats source can supply it.
    let stats: GenerateStats = parsed
        .timings
        .map(|t| t.stats(finish_reason.clone()))
        .unwrap_or_else(|| from_usage(parsed.usage, finish_reason));
    let msg = choice.message;
    let tool_calls = msg
        .tool_calls
        .into_iter()
        .map(|tc| NativeToolCall { name: tc.function.name, args: normalize_args(tc.function.arguments) })
        .collect();
    Ok(ChatResult { tool_calls, content: msg.content.unwrap_or_default(), stats })
}

pub async fn chat_with_tools(
    endpoint: &str,
    model: &str,
    system: &str,
    user: &str,
    tools: &Value,
    options: Option<GenerateOptions>,
) -> AppResult<ChatResult> {
    let client = streaming_client()?;
    let o = options.filter(|o| !o.is_empty()).unwrap_or_default();
    let body = ChatRequest {
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
    let resp = client
        .post(format!("{endpoint}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                AppError::Timeout(format!("connect to llama-server: {e}"))
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
#[path = "llama_chat_tests.rs"]
mod tests;

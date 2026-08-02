use crate::errors::{AppError, AppResult};
use crate::inference::backend::remote_guard::credential_allowed;
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::http::http::{body_or_note, streaming_client};
use crate::inference::http::ndjson::next_line;
use crate::inference::openai::chat_chunk::{strip_sse, ChatChunk, Usage};
use crate::inference::openai::chat_request::ChatRequest;
use crate::inference::openai::chat_stats::from_usage;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

/// Stream a generation from an OpenAI-compatible `/v1/chat/completions` endpoint
/// (mlx_lm.server, vLLM, SGLang — all SSE, all multi-model so `model` is in the
/// body). Token text flows through `on_token`; the call returns when a choice
/// reports `finish_reason`, the stream sends `[DONE]`, or `cancel` fires. When
/// `api_key` is `Some`, an `Authorization: Bearer` header is attached (remote
/// vLLM/SGLang started with `--api-key`); local mlx_lm.server passes `None`.
#[allow(clippy::too_many_arguments)]
pub async fn stream_generate(
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    options: Option<GenerateOptions>,
    think: Option<bool>,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> AppResult<GenerateStats> {
    let client = streaming_client()?;
    let body = ChatRequest::new(
        model.to_string(),
        prompt.to_string(),
        system,
        options.filter(|o| !o.is_empty()),
        think,
    );
    let mut req = client.post(format!("{endpoint}/v1/chat/completions")).json(&body);
    // Attach the key only over a safe channel (rule 7d); the save guardrail already blocks
    // storing a key on a cleartext remote URL — this is defense-in-depth for any other path.
    if let Some(key) = api_key.filter(|k| !k.is_empty() && credential_allowed(endpoint)) {
        req = req.bearer_auth(key);
    }
    // Race the request against cancel: a wedged server (e.g. a non-chat model
    // loaded) can accept the connection but never send response headers, which
    // would block `.send()` indefinitely — so Cancel must interrupt here too,
    // not only inside the streaming loop below.
    let send = req.send();
    let resp = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Ok(GenerateStats::default()),
        r = send => r.map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                AppError::Timeout(format!("connect to chat endpoint: {e}"))
            } else {
                AppError::Inference(e.to_string())
            }
        })?,
    };

    let status = resp.status();
    if !status.is_success() {
        let body_text = body_or_note(resp).await;
        return Err(AppError::Inference(format!("chat HTTP {status}: {body_text}")));
    }

    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut usage: Option<Usage> = None;
    // Carry "stop" vs "length" so the agentic runner can tell a real failure from a
    // `num_predict` truncation it can retry. Recorded (not returned) when it arrives —
    // see the finish_reason handling below.
    let mut finish: Option<String> = None;
    // A reasoning model streams its scratchpad in `delta.reasoning`/`reasoning_content` (not
    // `content`). Re-emit it as inline `<think>…</think>` so the runner's `strip_think` + D9
    // accounting handle every OpenAI-wire backend identically to llama.cpp. `think_open`
    // tracks the open tag: closed when the answer (`content`) starts or the stream ends. A terse
    // model (or enable_thinking:false) sends no `reasoning`, so this is a no-op.
    let mut think_open = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(GenerateStats::default()),
            piece = bytes.next() => {
                let Some(piece) = piece else { break };
                let piece = piece.map_err(|e| AppError::Inference(e.to_string()))?;
                buf.extend_from_slice(&piece);
                while let Some(line) = next_line(&mut buf) {
                    let payload = strip_sse(&line);
                    if payload.is_empty() { continue; }
                    if payload == b"[DONE]" {
                        if think_open { on_token("</think>"); }
                        return Ok(finalize(usage, finish));
                    }
                    // Skip SSE keep-alive/comment (": ...") and any non-data
                    // framing (event:/id:); only JSON-object chunks are parsed.
                    if payload.first() != Some(&b'{') { continue; }
                    let chunk: ChatChunk = serde_json::from_slice(payload)
                        .map_err(|e| AppError::Inference(format!("bad chunk: {e}")))?;
                    if chunk.usage.is_some() { usage = chunk.usage; }
                    if let Some(choice) = chunk.choices.into_iter().next() {
                        if let Some(text) = choice.delta.reasoning.filter(|t| !t.is_empty()) {
                            if !think_open { on_token("<think>"); think_open = true; }
                            on_token(&text);
                        }
                        if let Some(text) = choice.delta.content.filter(|t| !t.is_empty()) {
                            if think_open { on_token("</think>"); think_open = false; }
                            on_token(&text);
                        }
                        if cancel.is_cancelled() { return Ok(GenerateStats::default()); }
                        if let Some(fr) = choice.finish_reason {
                            // Record the stop reason but KEEP reading: with
                            // `stream_options.include_usage`, vLLM/SGLang send `usage` in a
                            // SEPARATE trailing chunk (choices:[]) AFTER this one, so returning
                            // here would drop the token counts. Finalize on `[DONE]` / stream end.
                            // (mlx_lm.server puts usage on this same chunk, then sends `[DONE]` —
                            // still captured, no regression.)
                            if think_open { on_token("</think>"); think_open = false; }
                            finish = Some(fr);
                        }
                    }
                }
            }
        }
    }
    if think_open { on_token("</think>"); }
    Ok(finalize(usage, finish))
}

/// Assemble the terminal stats from the (possibly trailing) `usage` chunk and the
/// recorded stop reason.
fn finalize(usage: Option<Usage>, finish: Option<String>) -> GenerateStats {
    from_usage(usage, finish)
}

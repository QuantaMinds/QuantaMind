use crate::errors::{AppError, AppResult};
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::http::http::{body_or_note, streaming_client};
use crate::inference::http::ndjson::next_line;
use crate::inference::llama::llama_wire::{
    llama_error_hint, strip_sse, ChatRequest, ChatStreamChunk, CompletionChunk,
    CompletionRequest,
};
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

/// Stream a generation from the bundled `llama-server`. Token text flows through
/// `on_token`; the call returns when the model stops or `cancel` fires.
///
/// PRIMARY path is the templated `/v1/chat/completions` endpoint: with `--jinja`
/// at spawn the server applies the GGUF's embedded chat template, so the model
/// sees its trained turn structure, emits EOS, and stops. The legacy
/// `/completion` endpoint (raw prompt, NO template — the original infinite-loop
/// path) is kept only as a 404 fallback for older builds. If neither route
/// exists, the server on this port almost certainly isn't ours (its `/health`
/// can still 200) — surface the likely port collision.
pub async fn stream_generate(
    endpoint: &str,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    options: Option<GenerateOptions>,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> AppResult<GenerateStats> {
    let opts = options.filter(|o| !o.is_empty());
    if let Some(stats) = stream_chat(
        endpoint,
        model,
        prompt,
        system,
        opts.clone(),
        cancel.clone(),
        &mut on_token,
    )
    .await?
    {
        return Ok(stats);
    }
    if let Some(stats) =
        stream_completion(endpoint, prompt, system, opts, cancel, &mut on_token).await?
    {
        return Ok(stats);
    }
    Err(AppError::Inference(format!(
        "llama.cpp inference failed on {endpoint}: neither /v1/chat/completions nor \
         /completion is available. Another server is likely on this port — e.g. \
         mlx_lm.server (default 8080). Stop it (or run it on 8082), then Stop & Start llama.cpp."
    )))
}

/// Templated chat path. `Ok(None)` means the route 404'd (try the fallback);
/// any other failure propagates. A cancel mid-flight returns default stats.
async fn stream_chat(
    endpoint: &str,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    opts: Option<GenerateOptions>,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> AppResult<Option<GenerateStats>> {
    let client = streaming_client()?;
    let body = ChatRequest::new(model.to_string(), prompt.to_string(), system, opts);
    let url = format!("{endpoint}/v1/chat/completions");
    // Race the request against cancel: a wedged server can accept the connection
    // but never send headers, blocking `.send()` — so Cancel must interrupt here.
    let send = client.post(&url).json(&body).send();
    let resp = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Ok(Some(GenerateStats::default())),
        r = send => r.map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                AppError::Timeout(format!("connect to llama-server {url}: {e}"))
            } else {
                AppError::Inference(format!("llama-server POST {url}: {e}"))
            }
        })?,
    };
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        let body_text = body_or_note(resp).await;
        // A context overflow or GPU compute error (Metal OOM) gets actionable copy
        // (raise/lower Context window, restart, smaller model); anything else keeps the
        // self-explaining URL+status+body.
        let msg = llama_error_hint(&body_text)
            .unwrap_or_else(|| format!("llama-server POST {url} → HTTP {status}: {body_text}"));
        return Err(AppError::Inference(msg));
    }

    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    // llama-server reports per-phase ms in a `timings` extension on the final
    // chunk — keep the latest so prefill/predict ms reach GenerateStats (and the
    // Inspector's TTFT breakdown), which token-count-only `usage` can't give.
    let mut timings = None;
    // Modern llama-server streams a reasoning model's scratchpad in `delta.reasoning_content` (not
    // `content`) BEFORE the answer. Re-emit it as an inline `<think>…</think>` block so the runner's
    // `strip_think` + D9 accounting handle llama.cpp identically to Ollama. `think_open` tracks the
    // open tag: closed when the answer (`content`) starts or the stream ends. A terse model (or
    // `--reasoning-format none`) sends no `reasoning_content`, so this stays a no-op.
    let mut think_open = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(Some(GenerateStats::default())),
            piece = bytes.next() => {
                let Some(piece) = piece else { break };
                let piece = piece.map_err(|e| AppError::Inference(e.to_string()))?;
                buf.extend_from_slice(&piece);
                while let Some(line) = next_line(&mut buf) {
                    let payload = strip_sse(&line);
                    if payload.is_empty() { continue; }
                    if payload == b"[DONE]" {
                        if think_open { on_token("</think>"); }
                        return Ok(Some(chat_stats(timings)));
                    }
                    if payload.first() != Some(&b'{') { continue; }
                    let chunk: ChatStreamChunk = serde_json::from_slice(payload)
                        .map_err(|e| AppError::Inference(format!("bad chunk: {e}")))?;
                    if chunk.timings.is_some() { timings = chunk.timings; }
                    if let Some(choice) = chunk.choices.into_iter().next() {
                        if let Some(text) = choice.delta.reasoning_content.filter(|t| !t.is_empty()) {
                            if !think_open { on_token("<think>"); think_open = true; }
                            on_token(&text);
                        }
                        if let Some(text) = choice.delta.content.filter(|t| !t.is_empty()) {
                            if think_open { on_token("</think>"); think_open = false; }
                            on_token(&text);
                        }
                        if cancel.is_cancelled() { return Ok(Some(GenerateStats::default())); }
                        if choice.finish_reason.is_some() {
                            if think_open { on_token("</think>"); }
                            // Carry "stop" vs "length" so the agentic runner can tell a real
                            // failure from a `num_predict` truncation it can retry (see runner).
                            let mut stats = chat_stats(timings);
                            stats.finish_reason = choice.finish_reason;
                            return Ok(Some(stats));
                        }
                    }
                }
            }
        }
    }
    if think_open { on_token("</think>"); }
    Ok(Some(chat_stats(timings)))
}

/// Stats for a chat-endpoint run: llama-server's `timings` (prompt/predict ms)
/// when present, else the all-`None` default — never fabricated. `finish_reason` is left
/// `None` here BY DESIGN and stamped by the streaming caller from the chunk's
/// `choice.finish_reason` as chunks arrive (see the `stats.finish_reason = ...` below) —
/// this helper never sees a choice.
fn chat_stats(timings: Option<crate::inference::llama::llama_timings::Timings>) -> GenerateStats {
    timings.map(|t| t.stats(None)).unwrap_or_default()
}

/// Legacy `/completion` fallback (raw prompt, no chat template). `Ok(None)` means
/// the route 404'd. System text is prepended to the prompt, as this endpoint
/// applies no template.
async fn stream_completion(
    endpoint: &str,
    prompt: &str,
    system: Option<&str>,
    opts: Option<GenerateOptions>,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> AppResult<Option<GenerateStats>> {
    let client = streaming_client()?;
    let full = match system {
        Some(s) if !s.is_empty() => format!("{s}\n\n{prompt}"),
        _ => prompt.to_string(),
    };
    let body = CompletionRequest::new(full, opts);
    let url = format!("{endpoint}/completion");
    let resp = client.post(&url).json(&body).send().await.map_err(|e| {
        if e.is_timeout() || e.is_connect() {
            AppError::Timeout(format!("connect to llama-server {url}: {e}"))
        } else {
            AppError::Inference(format!("llama-server POST {url}: {e}"))
        }
    })?;

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        let body_text = body_or_note(resp).await;
        // A context overflow or GPU compute error (Metal OOM) gets actionable copy
        // (raise/lower Context window, restart, smaller model); anything else keeps the
        // self-explaining URL+status+body.
        let msg = llama_error_hint(&body_text)
            .unwrap_or_else(|| format!("llama-server POST {url} → HTTP {status}: {body_text}"));
        return Err(AppError::Inference(msg));
    }

    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(Some(GenerateStats::default())),
            piece = bytes.next() => {
                let Some(piece) = piece else { break };
                let piece = piece.map_err(|e| AppError::Inference(e.to_string()))?;
                buf.extend_from_slice(&piece);
                while let Some(line) = next_line(&mut buf) {
                    let payload = strip_sse(&line);
                    if payload.is_empty() || payload == b"[DONE]" { continue; }
                    let chunk: CompletionChunk = serde_json::from_slice(payload)
                        .map_err(|e| AppError::Inference(format!("bad chunk: {e}")))?;
                    if !chunk.content.is_empty() {
                        on_token(&chunk.content);
                    }
                    if cancel.is_cancelled() { return Ok(Some(GenerateStats::default())); }
                    if chunk.stop {
                        // The legacy `/completion` wire has no `choices`, so no stop reason
                        // exists to report — `None` is the measurement, not an omission.
                        return Ok(Some(chunk.timings.unwrap_or_default().stats(None)));
                    }
                }
            }
        }
    }
    Ok(Some(GenerateStats::default()))
}

/// The context window (`n_ctx`) the running llama-server was launched with — read from `/props`
/// (`default_generation_settings.n_ctx`, with a bare `n_ctx` fallback for older builds). llama.cpp
/// fixes this at launch and IGNORES per-request `num_ctx`, so the eval must size its `num_ctx` to
/// THIS actual window (which `plan_launch` may have RAM-clamped below the requested ceiling), or
/// the truncation-retry headroom math over-grants budget the runtime can't hold. `None` when the
/// server is unreachable / the field is absent — the caller falls back to the hardware-class band.
pub async fn probe_llama_n_ctx(endpoint: &str) -> Option<u32> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).build().ok()?;
    let resp = client.get(format!("{endpoint}/props")).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    v.get("default_generation_settings")
        .and_then(|g| g.get("n_ctx"))
        .or_else(|| v.get("n_ctx"))
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .filter(|&n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    /// A reasoning model on modern llama-server streams its scratchpad in `delta.reasoning_content`
    /// (extracted out of `content`). We must re-wrap it inline as `<think>…</think>` so `strip_think`
    /// + D9 accounting see it — identical to the Ollama `thinking` field. Without this the reasoning
    /// is silently dropped on llama.cpp (proven live: qwen3.5 emitted 187 reasoning_content chunks,
    /// 0 captured before this fix).
    #[tokio::test]
    async fn reasoning_content_is_rewrapped_inline_as_think() {
        let mut s = Server::new_async().await;
        let _m = s.mock("POST", "/v1/chat/completions").with_status(200)
            .with_body(concat!(
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"let me think\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\" harder\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"name\\\":\\\"go\\\"}\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            ))
            .create_async().await;
        let mut out = String::new();
        stream_generate(&s.url(), "m", "hi", None, None, CancellationToken::new(), |t| out.push_str(t)).await.unwrap();
        // Reasoning is captured, wrapped, and CLOSED before the answer — so strip_think leaves just the call.
        assert_eq!(out, "<think>let me think harder</think>{\"name\":\"go\"}");
    }

    /// `/props` → the launched `n_ctx` (from `default_generation_settings`), the actual window the
    /// eval must clamp to. Falls back to a bare `n_ctx`; `None` when absent or the server errors.
    #[tokio::test]
    async fn probe_n_ctx_reads_the_launched_window_from_props() {
        let mut s = Server::new_async().await;
        let _m = s.mock("GET", "/props").with_status(200)
            .with_body(r#"{"default_generation_settings":{"n_ctx":16384},"total_slots":1}"#)
            .create_async().await;
        assert_eq!(probe_llama_n_ctx(&s.url()).await, Some(16384));
    }

    #[tokio::test]
    async fn probe_n_ctx_falls_back_to_bare_field_and_none_on_error() {
        let mut s = Server::new_async().await;
        let _bare = s.mock("GET", "/props").with_status(200).with_body(r#"{"n_ctx":8192}"#).create_async().await;
        assert_eq!(probe_llama_n_ctx(&s.url()).await, Some(8192));
        let mut s2 = Server::new_async().await;
        let _err = s2.mock("GET", "/props").with_status(500).create_async().await;
        assert_eq!(probe_llama_n_ctx(&s2.url()).await, None, "unreachable/error → None → caller uses the band");
    }

    /// The templated /v1/chat/completions endpoint is PRIMARY — when it answers,
    /// /completion is never hit (no `expect` on it, so a stray call would 501).
    #[tokio::test]
    async fn chat_endpoint_is_primary() {
        let mut s = Server::new_async().await;
        let _chat = s
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n")
            .create_async()
            .await;
        let _completion = s.mock("POST", "/completion").expect(0).create_async().await;
        let mut out = String::new();
        stream_generate(
            &s.url(),
            "m",
            "p",
            None,
            None,
            CancellationToken::new(),
            |t| out.push_str(t),
        )
        .await
        .unwrap();
        assert_eq!(out, "hi");
        _completion.assert_async().await;
    }

    /// The chat endpoint's `timings` extension on the final chunk must populate
    /// per-phase stats (prefill ms) — the regression that blanked the Inspector's
    /// TTFT breakdown for llama.cpp when the chat endpoint became primary.
    #[tokio::test]
    async fn chat_stream_timings_populate_prefill_stats() {
        let mut s = Server::new_async().await;
        let _chat = s
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_body("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"timings\":{\"prompt_n\":12,\"prompt_ms\":210.7,\"predicted_n\":5,\"predicted_ms\":99.0}}\n\ndata: [DONE]\n\n")
            .create_async()
            .await;
        let stats = stream_generate(&s.url(), "m", "p", None, None, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(stats.prompt_eval_ms, Some(211), "prefill ms from timings (rounded)");
        assert_eq!(stats.prompt_eval_count, Some(12));
        assert_eq!(stats.eval_count, Some(5));
    }

    /// When the chat route 404s (older build), fall back to legacy /completion so
    /// the run still works.
    #[tokio::test]
    async fn falls_back_to_completion_when_chat_404s() {
        let mut s = Server::new_async().await;
        let _chat = s
            .mock("POST", "/v1/chat/completions")
            .with_status(404)
            .create_async()
            .await;
        let _c = s
            .mock("POST", "/completion")
            .with_status(200)
            .with_body("data: {\"content\":\"hi\",\"stop\":false}\n\ndata: {\"content\":\"\",\"stop\":true}\n\n")
            .create_async()
            .await;
        let mut out = String::new();
        stream_generate(
            &s.url(),
            "m",
            "p",
            None,
            None,
            CancellationToken::new(),
            |t| out.push_str(t),
        )
        .await
        .unwrap();
        assert_eq!(out, "hi");
    }

    /// When BOTH routes 404, the error points at the likely port collision
    /// (another server shadowing llama-server).
    #[tokio::test]
    async fn both_endpoints_404_surfaces_port_collision_hint() {
        let mut s = Server::new_async().await;
        let _chat = s
            .mock("POST", "/v1/chat/completions")
            .with_status(404)
            .create_async()
            .await;
        let _c = s
            .mock("POST", "/completion")
            .with_status(404)
            .create_async()
            .await;
        let err = stream_generate(
            &s.url(),
            "m",
            "p",
            None,
            None,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Another server is likely on"),
            "should hint at the collision: {msg}"
        );
    }

    /// A 400 context-overflow on the chat route is rewritten into actionable copy
    /// (raise Context window + restart), not the raw JSON — the cure for the
    /// llama.cpp fixed-context wall.
    #[tokio::test]
    async fn context_overflow_400_yields_actionable_message() {
        let mut s = Server::new_async().await;
        let _m = s
            .mock("POST", "/v1/chat/completions")
            .with_status(400)
            .with_body(r#"{"error":{"code":400,"message":"request (10536 tokens) exceeds the available context size (8192 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":10536,"n_ctx":8192}}"#)
            .create_async()
            .await;
        let err = stream_generate(&s.url(), "m", "p", None, None, CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Context window"), "should tell the user which param to raise: {msg}");
        assert!(msg.contains("10536 tokens"), "should name the prompt size: {msg}");
        assert!(!msg.contains("exceed_context_size_error"), "should not dump the raw type: {msg}");
    }

    /// A non-404 error on the primary chat route is surfaced with URL + status +
    /// body (no fallback) so the failure is self-explaining.
    #[tokio::test]
    async fn non_404_error_names_the_url_status_and_body() {
        let mut s = Server::new_async().await;
        let _m = s
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;
        let err = stream_generate(
            &s.url(),
            "m",
            "p",
            None,
            None,
            CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("/v1/chat/completions"),
            "should name the URL: {msg}"
        );
        assert!(msg.contains("500"), "should name the status: {msg}");
        assert!(msg.contains("boom"), "should include the body: {msg}");
    }
}

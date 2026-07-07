use crate::errors::{AppError, AppResult};
use crate::inference::http::http::{body_or_note, streaming_client};
pub use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::generate::generate_stats::GenerateStats;
use crate::inference::ollama::ollama_wire::{GenerateChunk, GenerateRequest};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

#[allow(clippy::too_many_arguments)]
pub async fn stream_generate(
    endpoint: &str,
    model: &str,
    prompt: &str,
    system: Option<&str>,
    options: Option<GenerateOptions>,
    keep_alive: Option<i32>,
    think: Option<bool>,
    cancel: CancellationToken,
    mut on_token: impl FnMut(&str),
) -> AppResult<GenerateStats> {
    let client = streaming_client()?;
    let options = options.filter(|o| !o.is_empty());
    let body = GenerateRequest { model, prompt, system, options, keep_alive, think, stream: true };
    // Raced against `cancel` too, not just the streaming loop below: Ollama doesn't send response
    // headers until it has loaded the model AND finished prefilling the prompt — on a cold model
    // (not yet resident) or a long transcript that can take seconds, during which a Stop click
    // would otherwise sit inert until the first byte finally arrives.
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Ok(GenerateStats::default()),
        r = client.post(format!("{endpoint}/api/generate")).json(&body).send() => r.map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                AppError::Timeout(format!("connect to Ollama: {e}"))
            } else {
                AppError::Inference(e.to_string())
            }
        })?,
    };

    let status = resp.status();
    if !status.is_success() {
        let body_text = body_or_note(resp).await;
        return Err(AppError::Inference(format!("generate HTTP {status}: {body_text}")));
    }

    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    // When `think:true`, Ollama streams the scratchpad in `thinking` (not `response`) BEFORE the
    // answer. We re-emit it as an inline `<think>…</think>` block so the runner's `strip_think`
    // handles Ollama identically to llama.cpp. `think_open` tracks the open tag: it's closed when
    // the answer starts OR at `done` — the latter is the truncation case (reasoning ate the whole
    // `num_predict`, `response` empty), where closing keeps the captured reasoning well-formed.
    let mut think_open = false;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(GenerateStats::default()),
            piece = bytes.next() => {
                let Some(piece) = piece else { break };
                let piece = piece.map_err(|e| AppError::Inference(e.to_string()))?;
                buf.extend_from_slice(&piece);
                while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=nl).collect();
                    let trimmed = &line[..line.len() - 1];
                    if trimmed.is_empty() { continue; }
                    let chunk: GenerateChunk = serde_json::from_slice(trimmed)
                        .map_err(|e| AppError::Inference(format!("bad chunk: {e}")))?;
                    if !chunk.thinking.is_empty() {
                        if !think_open { on_token("<think>"); think_open = true; }
                        on_token(&chunk.thinking);
                    }
                    if !chunk.response.is_empty() {
                        if think_open { on_token("</think>"); think_open = false; }
                        on_token(&chunk.response);
                    }
                    if cancel.is_cancelled() { return Ok(GenerateStats::default()); }
                    if chunk.done {
                        if think_open { on_token("</think>"); }
                        return Ok(chunk.stats());
                    }
                }
            }
        }
    }
    if think_open { on_token("</think>"); }
    Ok(GenerateStats::default())
}

const UNLOAD_POLL: Duration = Duration::from_millis(500);
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Deserialize)]
struct PsBody {
    #[serde(default)]
    models: Vec<PsEntry>,
}

#[derive(Deserialize)]
struct PsEntry {
    name: String,
    #[serde(default)]
    size_vram: u64,
}

/// VRAM bytes Ollama reports for `model` right now (`/api/ps`), or 0 when it's not
/// loaded. The oracle for the VRAM-isolation gate. A lean GET kept here (not the
/// `commands`-layer `fetch_loaded`) so `inference/` stays free of `commands`.
async fn vram_for(endpoint: &str, model: &str) -> AppResult<u64> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let resp = client
        .get(format!("{endpoint}/api/ps"))
        .send()
        .await
        .map_err(|e| AppError::Inference(format!("ps: {e}")))?;
    if !resp.status().is_success() {
        return Ok(0);
    }
    let body: PsBody = resp.json().await.map_err(|e| AppError::Inference(format!("ps body: {e}")))?;
    Ok(body.models.iter().find(|m| m.name == model).map(|m| m.size_vram).unwrap_or(0))
}

/// Ask Ollama to unload `model` immediately (`keep_alive: 0`). Best-effort — the
/// `/api/ps` poll below is the real gate; this just triggers the eviction.
async fn request_unload(endpoint: &str, model: &str) -> AppResult<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let _ = client
        .post(format!("{endpoint}/api/generate"))
        .json(&json!({ "model": model, "keep_alive": 0, "stream": false }))
        .send()
        .await;
    Ok(())
}

/// Hard VRAM-isolation gate: evict `model` and poll `/api/ps` until its VRAM is 0.
/// **Assert-and-fail** — returns `Err` if the VRAM hasn't released within the
/// timeout. The caller MUST halt rather than load the next model onto dirty VRAM
/// (the exact OOM lock-up this prevents). Never a silent "best-effort" continue.
pub async fn force_unload(endpoint: &str, model: &str) -> AppResult<()> {
    force_unload_inner(endpoint, model, UNLOAD_POLL, UNLOAD_TIMEOUT).await
}

async fn force_unload_inner(endpoint: &str, model: &str, poll: Duration, timeout: Duration) -> AppResult<()> {
    let _ = request_unload(endpoint, model).await;
    let start = Instant::now();
    loop {
        if vram_for(endpoint, model).await? == 0 {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(AppError::Inference(format!(
                "VRAM for '{model}' did not release within {}s — run paused to avoid an OOM load",
                timeout.as_secs()
            )));
        }
        sleep(poll).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    #[tokio::test]
    #[ignore = "live: hits a real Ollama on :11434 — proves Stop actually aborts an in-flight \
                generation quickly, not just at the next turn boundary. Run explicitly: \
                cargo test --lib inference::ollama::ollama::tests::live_cancel -- --ignored --nocapture"]
    async fn live_cancel_aborts_a_long_generation_within_a_second_not_at_completion() {
        // The mechanism the whole Stop-Batch latency fix leans on: `stream_generate`'s
        // `tokio::select!` races the HTTP stream against `cancel`. Never verified against a REAL
        // server before — a mock can't prove the actual TCP stream is abandoned promptly rather
        // than silently drained in the background while the caller moves on.
        let model = "ornith-1.0-35b-Q8_0:latest";
        // Guarantee THIS test's scenario (a warm, already-streaming model) regardless of what
        // state a previous/concurrent test left the server in — the sibling cold-load test below
        // covers the other scenario explicitly instead of leaving it to leftover ordering.
        let mut warm = String::new();
        stream_generate(
            "http://localhost:11434", model, "hi", None,
            Some(GenerateOptions { num_predict: Some(1), temperature: Some(0.0), ..Default::default() }),
            None, None, CancellationToken::new(), |t| warm.push_str(t),
        )
        .await
        .expect("warm-up call failed");

        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            cancel2.cancel();
        });

        let start = Instant::now();
        let mut tokens = 0usize;
        let stats = stream_generate(
            "http://localhost:11434",
            model,
            "Write a very long, detailed 2000-word short story about a dragon who learns to bake bread. \
             Do not stop early — write the full story.",
            None,
            Some(GenerateOptions { num_predict: Some(4096), temperature: Some(0.0), ..Default::default() }),
            None,
            None,
            cancel,
            |_t| tokens += 1,
        )
        .await
        .expect("a cancelled stream returns Ok(default stats), never an Err");
        let elapsed = start.elapsed();

        eprintln!("live_cancel: elapsed={elapsed:?} tokens_streamed={tokens} stats={stats:?}");
        // Generous margin over the network hop (loopback) and Ollama's own poll granularity —
        // still an order of magnitude below "ran to completion" (a 2000-word story at any local
        // token rate is many seconds, often 30s+ on a 35B Q8 model).
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?} — the abort is not prompt");
        assert!(tokens > 0, "the warm-up guarantees streaming had already started — this run's scenario");
        assert_eq!(stats.eval_count, None, "the cancel path returns GenerateStats::default(), never real usage");
    }

    #[tokio::test]
    #[ignore = "live: hits a real Ollama on :11434, forces the model OFF VRAM first — proves Stop \
                aborts promptly even during a cold load + prefill, not just once streaming has \
                already started. Run explicitly: cargo test --lib \
                inference::ollama::ollama::tests::live_cancel_cold -- --ignored --nocapture"]
    async fn live_cancel_cold_load_aborts_promptly_not_after_the_model_finishes_loading() {
        // Without racing the initial `send()` against `cancel` (see the fix above this test),
        // a click landing DURING the cold model-load + prompt-prefill window — before Ollama
        // sends the first response byte — sat inert: measured 8s on this same 35B model before
        // the fix, entirely before any token streamed. Force the model off VRAM so this test
        // reliably re-triggers that exact window instead of hitting an already-warm model.
        let model = "ornith-1.0-35b-Q8_0:latest";
        force_unload("http://localhost:11434", model).await.expect("could not force-unload for the test");

        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            cancel2.cancel();
        });

        let start = Instant::now();
        let mut tokens = 0usize;
        let stats = stream_generate(
            "http://localhost:11434",
            model,
            "Write a very long, detailed 2000-word short story about a dragon who learns to bake bread.",
            None,
            Some(GenerateOptions { num_predict: Some(4096), temperature: Some(0.0), ..Default::default() }),
            None,
            None,
            cancel,
            |_t| tokens += 1,
        )
        .await
        .expect("a cancelled stream returns Ok(default stats), never an Err");
        let elapsed = start.elapsed();

        eprintln!("live_cancel_cold: elapsed={elapsed:?} tokens_streamed={tokens} stats={stats:?}");
        // The cancel fires at 300ms, well before an 8-second-class cold load could ever complete —
        // a generous ceiling that still proves the fix, without being tight enough to flake on a
        // slower box's model-load time.
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?} — cold-load cancel is not prompt");
        assert_eq!(tokens, 0, "cancelled before the model produced a single token");
    }

    #[tokio::test]
    async fn force_unload_sends_keep_alive_zero_and_returns_when_vram_clears() {
        let mut s = Server::new_async().await;
        let unload = s
            .mock("POST", "/api/generate")
            .match_body(Matcher::PartialJson(json!({ "keep_alive": 0 })))
            .with_status(200)
            .with_body(r#"{"done":true}"#)
            .create_async()
            .await;
        let _ps = s
            .mock("GET", "/api/ps")
            .with_status(200)
            .with_body(r#"{"models":[]}"#) // model already evicted
            .create_async()
            .await;
        let r = force_unload_inner(&s.url(), "qwen", Duration::from_millis(5), Duration::from_secs(1)).await;
        assert!(r.is_ok());
        unload.assert_async().await; // the keep_alive:0 body really went out
    }

    #[tokio::test]
    async fn force_unload_errors_when_vram_never_releases_never_silently_ok() {
        let mut s = Server::new_async().await;
        let _unload = s.mock("POST", "/api/generate").with_status(200).with_body("{}").create_async().await;
        let _ps = s
            .mock("GET", "/api/ps")
            .with_status(200)
            .with_body(r#"{"models":[{"name":"qwen","size_vram":9000000000}]}"#) // stuck in VRAM
            .create_async()
            .await;
        let r = force_unload_inner(&s.url(), "qwen", Duration::from_millis(5), Duration::from_millis(30)).await;
        assert!(r.is_err(), "a stuck unload must Err (halt), never a silent Ok that OOMs the next load");
    }
}

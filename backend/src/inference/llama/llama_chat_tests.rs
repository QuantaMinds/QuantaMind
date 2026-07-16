use super::parse_chat;
use serde_json::json;

#[test]
fn parses_tool_calls_with_object_arguments() {
    // llama.cpp builds that hand back `arguments` as a real object.
    let body = json!({
        "choices": [{ "message": {
            "content": "",
            "tool_calls": [{ "function": { "name": "get_weather", "arguments": { "city": "Paris" } } }]
        }}],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    })
    .to_string();
    let r = parse_chat(&body).unwrap();
    assert_eq!(r.tool_calls.len(), 1);
    assert_eq!(r.tool_calls[0].name, "get_weather");
    assert_eq!(r.tool_calls[0].args, json!({ "city": "Paris" }));
}

#[test]
fn parses_tool_calls_with_stringified_arguments() {
    // The OpenAI-spec shape: `arguments` is a JSON string — normalize_args must
    // parse it back to an object so checkpoint/arg matching compares objects.
    let body = json!({
        "choices": [{ "message": {
            "tool_calls": [{ "function": { "name": "run_tests", "arguments": "{\"module\": \"cart\"}" } }]
        }}]
    })
    .to_string();
    let r = parse_chat(&body).unwrap();
    assert_eq!(r.tool_calls.len(), 1);
    assert_eq!(r.tool_calls[0].args, json!({ "module": "cart" }));
}

#[test]
fn surfaces_plain_content_when_no_tool_calls() {
    let body = json!({ "choices": [{ "message": { "content": "Paris is the capital." } }] }).to_string();
    let r = parse_chat(&body).unwrap();
    assert!(r.tool_calls.is_empty());
    assert_eq!(r.content, "Paris is the capital.");
}

#[test]
fn empty_choices_is_a_clean_empty_result() {
    let r = parse_chat(&json!({ "choices": [] }).to_string()).unwrap();
    assert!(r.tool_calls.is_empty());
    assert_eq!(r.content, "");
}

#[test]
fn prefers_timings_over_usage_for_per_phase_stats() {
    // llama-server's `timings` (prompt/predict ms) must drive stats, not the
    // token-count-only `usage` — so the prefill ms reaches the Inspector.
    let body = json!({
        "choices": [{ "message": { "content": "hi" } }],
        "usage": { "prompt_tokens": 99 },
        "timings": { "prompt_n": 12, "prompt_ms": 210.7, "predicted_n": 5, "predicted_ms": 80.0 }
    })
    .to_string();
    let r = parse_chat(&body).unwrap();
    assert_eq!(r.stats.prompt_eval_ms, Some(211), "prefill ms present (rounded)");
    assert_eq!(r.stats.prompt_eval_count, Some(12), "from timings.prompt_n, not usage.prompt_tokens");
}

/// THE REGRESSION (#161). `Choice` had no `finish_reason` field at all, so a llama.cpp
/// native turn's `stats.finish_reason` was permanently `None`. That made TWO things in the
/// agentic runner dead code on this path: the truncation RETRY (`runner.rs:554`, so a turn
/// cut mid-tool-call was never retried with a bigger budget) and the setting-vs-hardware
/// split (`runner.rs:650`), whose own comment promises it exists "so it's never laundered
/// into Malformed/Hallucinated/EmptyOutput" — which is exactly what always happened.
///
/// So a model cut off by OUR output cap was recorded as HALLUCINATING. Worse, Ollama maps
/// `done_reason` → `finish_reason`, so the same model scored differently on the two backends
/// purely because of which struct decoded the reply — a decoder finding wearing a model's
/// face, and a cross-backend comparability break.
#[test]
fn a_truncated_tool_turn_reports_length_not_a_silent_none() {
    let body = serde_json::json!({
        "choices": [{
            "message": { "content": "{\"name\":\"run_tests\"", "tool_calls": [] },
            // The server SAYS it cut the turn off. Before the fix, nobody opened the envelope.
            "finish_reason": "length"
        }],
        "timings": { "prompt_n": 120, "prompt_ms": 30.0, "predicted_n": 256, "predicted_ms": 900.0 }
    })
    .to_string();
    let r = parse_chat(&body).unwrap();
    assert_eq!(
        r.stats.finish_reason.as_deref(),
        Some("length"),
        "the runner's truncation retry + Truncated/ReasoningOverrun split key off this",
    );
}

/// The stop reason must survive the OTHER stats arm too. `parse_chat` prefers `timings` and
/// falls back to `usage`; `finish_reason` comes from the choice, so NEITHER source supplies
/// it — threading it through only one arm would leave a hole that depends on whether the
/// server happened to send `timings`.
#[test]
fn the_stop_reason_survives_the_usage_fallback_arm_too() {
    let body = serde_json::json!({
        "choices": [{ "message": { "content": "hi", "tool_calls": [] }, "finish_reason": "length" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        // no `timings` → the from_usage arm
    })
    .to_string();
    let r = parse_chat(&body).unwrap();
    assert_eq!(r.stats.finish_reason.as_deref(), Some("length"));
}

/// A natural stop is reported as such, not conflated with truncation.
#[test]
fn a_natural_stop_reports_stop() {
    let body = serde_json::json!({
        "choices": [{ "message": { "content": "done", "tool_calls": [] }, "finish_reason": "stop" }],
        "timings": { "prompt_n": 5, "predicted_n": 2 }
    })
    .to_string();
    assert_eq!(parse_chat(&body).unwrap().stats.finish_reason.as_deref(), Some("stop"));
}

/// Absent → `None`. "Not reported" is a measurement, not a reason to invent one.
#[test]
fn an_absent_stop_reason_stays_none_never_fabricated() {
    let body = serde_json::json!({
        "choices": [{ "message": { "content": "x", "tool_calls": [] } }]
    })
    .to_string();
    assert_eq!(parse_chat(&body).unwrap().stats.finish_reason, None);
}

/// LIVE (#161). A fixture CANNOT catch this bug — you write the JSON from the same mental
/// model that omitted the field, so the test and the bug share an author and both go green.
/// Only a real server sending a real cut-off reply proves `"length"` arrives on the wire AND
/// survives into `stats`. Deliberately caps `max_tokens` low so the model is truncated
/// mid-tool-call: the exact turn that used to be scored `Hallucinated`.
///
/// Run: cargo test --lib live_llama_truncated_turn -- --ignored --nocapture
#[tokio::test]
#[ignore = "live: requires llama-server on :8080 (--jinja) with the model loaded"]
async fn live_llama_truncated_turn_reports_length() {
    use crate::inference::eval::toolcall::tasks::ToolSchema;
    use crate::inference::generate::generate_options::GenerateOptions;

    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5-coder".into());
    let tools = serde_json::json!([{
        "type": "function",
        "function": {
            "name": "write_essay",
            "description": "Write a long essay",
            "parameters": { "type": "object", "properties": { "text": { "type": "string" } } }
        }
    }]);
    let _ = ToolSchema { name: "x".into(), description: String::new(), parameters: serde_json::json!({}) };
    // 8 output tokens cannot fit a tool call — the server MUST stop on "length".
    let opts = GenerateOptions { temperature: Some(0.0), num_predict: Some(8), ..Default::default() };
    let r = super::chat_with_tools(
        "http://127.0.0.1:8080",
        &model,
        "You complete the task using the available tools.",
        "Write a 2000-word essay about databases using the write_essay tool.",
        &tools,
        Some(opts),
    )
    .await
    .expect("live llama.cpp tool call");

    println!("\n=== LIVE truncation: llama.cpp/{model} ===");
    println!("  finish_reason : {:?}", r.stats.finish_reason);
    println!("  eval_count    : {:?}  (capped at 8)", r.stats.eval_count);
    println!("  tool_calls    : {}", r.tool_calls.len());
    println!("  content       : {:?}", r.content.chars().take(60).collect::<String>());
    assert_eq!(
        r.stats.finish_reason.as_deref(),
        Some("length"),
        "a real capped turn must report `length` — this is what the runner's truncation retry \
         and its Truncated/ReasoningOverrun split key off. Before the fix it was always None, \
         so every truncation was laundered into Malformed/Hallucinated/EmptyOutput.",
    );
}

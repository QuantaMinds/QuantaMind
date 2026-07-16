use super::{parse_chat, ChatRequest};
use crate::inference::generate::generate_options::GenerateOptions;
use serde_json::json;

/// The `/api/chat` wire sends `think:false` when suppression is requested (a thinking-by-default
/// model must not burn the turn budget in a hidden scratchpad) and omits the field when `None`
/// (byte-identical to before the field existed).
#[test]
fn think_false_is_serialized_and_none_is_omitted() {
    let tools = json!([]);
    let suppressed = ChatRequest {
        model: "m", messages: vec![], tools: &tools, options: None, think: Some(false), stream: false,
    };
    assert!(serde_json::to_string(&suppressed).unwrap().contains("\"think\":false"));

    let default = ChatRequest {
        model: "m", messages: vec![], tools: &tools, options: None, think: None, stream: false,
    };
    assert!(!serde_json::to_string(&default).unwrap().contains("think"));
}

#[test]
fn parses_nested_object_arguments_with_embedded_quotes() {
    let body = r#"{"message":{"content":"","tool_calls":[
        {"function":{"name":"get_weather","arguments":{"city":"Paris \"Île\""}}}
    ]},"eval_count":12,"prompt_eval_count":30}"#;
    let r = parse_chat(body).unwrap();
    assert_eq!(r.tool_calls.len(), 1);
    assert_eq!(r.tool_calls[0].name, "get_weather");
    assert_eq!(r.tool_calls[0].args, json!({ "city": "Paris \"Île\"" }));
    assert_eq!(r.stats.eval_count, Some(12));
    assert_eq!(r.stats.prompt_eval_count, Some(30));
}

#[test]
fn normalizes_stringified_arguments_back_into_an_object() {
    // Some models return `arguments` as a JSON *string* — parse it to an object.
    let body = r#"{"message":{"tool_calls":[{"function":{"name":"cancel_order","arguments":"{\"order_id\":88}"}}]}}"#;
    let r = parse_chat(body).unwrap();
    assert_eq!(r.tool_calls[0].args, json!({ "order_id": 88 }));
}

#[test]
fn empty_tool_calls_yields_no_calls_plus_the_content() {
    let body = r#"{"message":{"content":"I cannot find that order."}}"#;
    let r = parse_chat(body).unwrap();
    assert!(r.tool_calls.is_empty());
    assert_eq!(r.content, "I cannot find that order.");
}

#[test]
fn parallel_tool_calls_all_parse() {
    let body = r#"{"message":{"tool_calls":[
        {"function":{"name":"a","arguments":{"x":1}}},
        {"function":{"name":"b","arguments":{"y":2}}},
        {"function":{"name":"c","arguments":{}}}
    ]}}"#;
    let r = parse_chat(body).unwrap();
    assert_eq!(r.tool_calls.len(), 3);
    assert_eq!(r.tool_calls[0].name, "a");
}

/// LIVE, the CROSS-BACKEND half of #161. Ollama always mapped `done_reason` → `finish_reason`;
/// llama.cpp's tool path never read the field at all. So the SAME model, truncated the SAME
/// way, reported `Some("length")` on one backend and `None` on the other — and the runner
/// scored the second as `Hallucinated`. That was a decoder difference wearing a model's face,
/// and it broke the repo's hard rule that a metric must never differ by run-mode.
///
/// This asserts Ollama's side of the parity. Its llama.cpp twin is
/// `llama_chat::tests::live_llama_truncated_turn_reports_length`; run both and they must now
/// agree.
///
/// Run: cargo test --lib live_ollama_truncated_turn -- --ignored --nocapture
#[tokio::test]
#[ignore = "live: requires Ollama on :11434 with a tool-capable model"]
async fn live_ollama_truncated_turn_reports_length() {
    let model = std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "qwen2.5:3b".into());
    let tools = serde_json::json!([{
        "type": "function",
        "function": {
            "name": "write_essay",
            "description": "Write a long essay",
            "parameters": { "type": "object", "properties": { "text": { "type": "string" } } }
        }
    }]);
    // 8 output tokens cannot fit a tool call — the server MUST stop on the cap.
    let opts = GenerateOptions { temperature: Some(0.0), num_predict: Some(8), ..Default::default() };
    let r = super::chat_with_tools(
        "http://127.0.0.1:11434",
        &model,
        "You complete the task using the available tools.",
        "Write a 2000-word essay about databases using the write_essay tool.",
        &tools,
        Some(opts),
        Some(false),
    )
    .await
    .expect("live Ollama tool call");

    println!("\n=== LIVE truncation: ollama/{model} ===");
    println!("  finish_reason : {:?}", r.stats.finish_reason);
    println!("  eval_count    : {:?}  (capped at 8)", r.stats.eval_count);
    assert_eq!(
        r.stats.finish_reason.as_deref(),
        Some("length"),
        "Ollama must report the cap — and llama.cpp must now agree, or the same model still \
         scores differently per backend for decoder reasons",
    );
}

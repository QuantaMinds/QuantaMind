use super::parse_chat;
use serde_json::json;

#[test]
fn parses_tool_calls_with_object_arguments() {
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
    // vLLM/SGLang report token counts only → counts present, per-phase ms stay None.
    assert_eq!(r.stats.prompt_eval_count, Some(10));
    assert!(r.stats.prompt_eval_ms.is_none());
}

#[test]
fn parses_tool_calls_with_stringified_arguments() {
    // OpenAI-spec shape: `arguments` is a JSON string — normalize_args parses it
    // back to an object so arg matching compares objects.
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

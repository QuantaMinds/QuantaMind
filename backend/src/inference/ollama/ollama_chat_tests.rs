use super::{parse_chat, ChatRequest};
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

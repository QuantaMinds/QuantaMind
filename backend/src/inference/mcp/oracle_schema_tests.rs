use super::*;
use serde_json::json;

fn read_tool() -> Tool {
    Tool {
        name: "read_text_file".into(),
        title: None,
        description: Some("read a file".into()),
        input_schema: json!({
            "type": "object",
            "properties": { "path": {"type": "string"}, "head": {"type": "number"} },
            "required": ["path"],
        }),
        output_schema: None,
        annotations: None,
    }
}

fn call(name: &str, args: Value) -> NativeToolCall {
    NativeToolCall { name: name.into(), args }
}

#[test]
fn valid_call_passes() {
    let check = check_call(&[read_tool()], &call("read_text_file", json!({"path": "/a"})));
    assert_eq!(check, CallCheck::Valid);
}

#[test]
fn namespaced_name_resolves_to_the_tool() {
    let check = check_call(&[read_tool()], &call("filesystem::read_text_file", json!({"path": "/a"})));
    assert!(check.is_valid());
}

#[test]
fn hallucinated_tool_is_unknown() {
    let check = check_call(&[read_tool()], &call("delete_everything", json!({})));
    assert_eq!(check, CallCheck::UnknownTool);
}

#[test]
fn missing_required_and_wrong_type_are_invalid() {
    let missing = check_call(&[read_tool()], &call("read_text_file", json!({"head": 5})));
    assert!(matches!(missing, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("required"))));

    let wrong = check_call(&[read_tool()], &call("read_text_file", json!({"path": 123})));
    assert!(matches!(wrong, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("expected string"))));
}

#[test]
fn additional_properties_false_rejects_hallucinated_args() {
    let strict = Tool {
        input_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false,
        }),
        ..read_tool()
    };
    let check = check_call(&[strict], &call("read_text_file", json!({"path": "/a", "sidenote": "exfil"})));
    assert!(matches!(check, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("unexpected property"))));
}

#[test]
fn nested_object_enum_and_array_items_are_validated() {
    let tool = Tool {
        name: "cfg".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["fast", "slow"]},
                "opts": {"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]},
                "tags": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["mode"],
        }),
        ..read_tool()
    };
    assert!(check_call(&[tool.clone()], &call("cfg", json!({"mode": "fast", "opts": {"n": 3}, "tags": ["a"]}))).is_valid());

    let bad_enum = check_call(&[tool.clone()], &call("cfg", json!({"mode": "turbo"})));
    assert!(matches!(bad_enum, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("enum"))));

    let bad_nested = check_call(&[tool.clone()], &call("cfg", json!({"mode": "fast", "opts": {"n": "three"}})));
    assert!(matches!(bad_nested, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("opts.n"))));

    let bad_item = check_call(&[tool], &call("cfg", json!({"mode": "fast", "tags": ["a", 2]})));
    assert!(matches!(bad_item, CallCheck::Invalid(v) if v.iter().any(|s| s.contains("tags[1]"))));
}

#[test]
fn schema_valid_rate_aggregates() {
    let tools = vec![read_tool()];
    let calls = vec![
        call("read_text_file", json!({"path": "/a"})),   // valid
        call("read_text_file", json!({"head": 1})),       // invalid (missing path)
        call("nope", json!({})),                            // unknown tool
    ];
    let score = score_calls(&tools, &calls);
    assert_eq!((score.total, score.valid, score.invalid, score.unknown_tool), (3, 1, 1, 1));
    assert!((score.rate() - 1.0 / 3.0).abs() < 1e-9);
}

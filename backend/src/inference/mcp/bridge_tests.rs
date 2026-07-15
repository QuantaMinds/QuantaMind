use super::*;
use crate::mcp::wire::{CallToolResult, ContentBlock, Tool};
use serde_json::json;

fn tool(name: &str) -> Tool {
    Tool {
        name: name.into(),
        title: None,
        description: Some("d".into()),
        input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        output_schema: None,
        annotations: None,
    }
}

#[test]
fn native_tools_shape_maps_input_schema_to_parameters() {
    let native = mcp_tools_to_native(&[tool("read_text_file")]);
    let f = &native[0]["function"];
    assert_eq!(native[0]["type"], "function");
    assert_eq!(f["name"], "read_text_file");
    assert_eq!(f["parameters"]["properties"]["path"]["type"], "string");
}

#[test]
fn capability_reliable_weak_unknown() {
    assert_eq!(assess_tool_capability("qwen2.5:7b"), ToolCapability::Reliable);
    assert_eq!(assess_tool_capability("llama3.1:8b"), ToolCapability::Reliable);
    assert_eq!(assess_tool_capability("qwen2.5:0.5b"), ToolCapability::Weak);
    assert_eq!(assess_tool_capability("gemma2:2b"), ToolCapability::Weak);
    assert_eq!(assess_tool_capability("some-random-model"), ToolCapability::Unknown);
    // 11b/70b must NOT be misread as small (1b).
    assert_eq!(assess_tool_capability("llama3.2:11b"), ToolCapability::Reliable);

    assert!(capability_warning("qwen2.5:7b").is_none());
    assert!(capability_warning("qwen2.5:0.5b").is_some());
    assert!(capability_warning("mystery-model").is_some());
}

#[test]
fn select_calls_prefers_native_then_falls_back_to_text() {
    let native = vec![NativeToolCall { name: "read_text_file".into(), args: json!({"path":"/a"}) }];
    assert_eq!(select_calls(&native, "ignored"), native, "native calls win");

    // No native calls → the model put the call in prose as JSON.
    let text = r#"I'll do it: {"name": "read_text_file", "arguments": {"path": "/a"}}"#;
    let parsed = select_calls(&[], text);
    assert_eq!(parsed.len(), 1, "text-embedded call recovered");
    assert_eq!(parsed[0].name, "read_text_file");
    assert_eq!(parsed[0].args["path"], "/a");

    // Nothing at all.
    assert!(select_calls(&[], "just chatting").is_empty());
}

#[test]
fn flatten_content_joins_text_and_marks_non_text() {
    let ok = CallToolResult {
        content: vec![ContentBlock::Text { text: "hello".into() }],
        is_error: None,
        structured_content: None,
    };
    assert_eq!(flatten_content(&ok), "hello");
    assert!(!ok.is_error());

    let img = CallToolResult {
        content: vec![ContentBlock::Image { data: "x".into(), mime_type: "image/png".into() }],
        is_error: Some(true),
        structured_content: None,
    };
    assert_eq!(flatten_content(&img), "[image content]");
    assert!(img.is_error());
}

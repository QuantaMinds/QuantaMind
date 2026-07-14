//! Phase 1 oracle: every real captured fixture in `fixtures/mcp/` must
//! round-trip through these types. The fixtures wrap each exchange as
//! `{transport, request, response}`; we pull the `request`/`response` payloads
//! and deserialize them into the typed structs. Bytes are compile-time embedded
//! via `include_str!` so the test never depends on a runtime cwd.

use super::*;
use serde_json::Value;

const INITIALIZE: &str = include_str!("../../../fixtures/mcp/initialize.json");
const TOOLS_LIST: &str = include_str!("../../../fixtures/mcp/tools_list.json");
const TOOLS_CALL: &str = include_str!("../../../fixtures/mcp/tools_call.json");
const ERR_INBAND: &str = include_str!("../../../fixtures/mcp/error_tool_inband.json");
const ERR_METHOD: &str = include_str!("../../../fixtures/mcp/error_method_not_found.json");

/// Pull the `request` / `response` sub-objects out of a fixture wrapper.
fn parts(fixture: &str) -> (Value, Value) {
    let w: Value = serde_json::from_str(fixture).expect("fixture is valid JSON");
    (w["request"].clone(), w["response"].clone())
}

// --- Step 1: envelopes ------------------------------------------------------

#[test]
fn response_success_and_error_are_distinct_arms() {
    // Protocol error fixture → Failure arm, code -32601.
    let (_, resp) = parts(ERR_METHOD);
    let r: Response = serde_json::from_value(resp).unwrap();
    match r.result() {
        Err(e) => {
            assert_eq!(e.code, code::METHOD_NOT_FOUND);
            assert_eq!(e.message, "Method not found");
        }
        Ok(_) => panic!("method-not-found must be the error arm"),
    }

    // A result-bearing response → Success arm.
    let (_, ok) = parts(TOOLS_CALL);
    let r: Response = serde_json::from_value(ok).unwrap();
    assert!(r.result().is_ok(), "a response with `result` is the success arm");
}

#[test]
fn response_roundtrips_both_arms() {
    for fx in [TOOLS_CALL, ERR_METHOD] {
        let (_, resp) = parts(fx);
        let typed: Response = serde_json::from_value(resp.clone()).unwrap();
        let back = serde_json::to_value(&typed).unwrap();
        let reparsed: Response = serde_json::from_value(back).unwrap();
        assert_eq!(typed, reparsed, "flatten+untagged must survive a round-trip");
    }
}

#[test]
fn request_id_accepts_number_and_string() {
    let n: RequestId = serde_json::from_str("7").unwrap();
    assert_eq!(n, RequestId::Num(7));
    let s: RequestId = serde_json::from_str("\"abc\"").unwrap();
    assert_eq!(s, RequestId::Str("abc".into()));
    // round-trip preserves the variant
    assert_eq!(serde_json::to_string(&n).unwrap(), "7");
    assert_eq!(serde_json::to_string(&s).unwrap(), "\"abc\"");
}

#[test]
fn outbound_request_serializes_compact_single_line() {
    let req = Request::new(
        RequestId::Num(3),
        method::TOOLS_CALL,
        Some(serde_json::json!({"name": "read_text_file", "arguments": {"path": "/tmp/x"}})),
    );
    let line = serde_json::to_string(&req).unwrap();
    assert!(!line.contains('\n'), "stdio framing forbids embedded newlines");
    assert!(line.contains("\"jsonrpc\":\"2.0\""));
    assert!(line.contains("\"method\":\"tools/call\""));
}

#[test]
fn notification_has_no_id_field() {
    let note = Notification::new(method::INITIALIZED, None);
    let v = serde_json::to_value(&note).unwrap();
    assert!(v.get("id").is_none(), "a notification must not carry an id");
    assert_eq!(v["method"], "notifications/initialized");
}

// --- Step 2: initialize -----------------------------------------------------

#[test]
fn initialize_request_parses_params() {
    let (req, _) = parts(INITIALIZE);
    let params: InitializeParams = serde_json::from_value(req["params"].clone()).unwrap();
    assert_eq!(params.protocol_version, LATEST_PROTOCOL_VERSION);
    assert_eq!(params.client_info.name, "quantamind-fixture-capture");
}

#[test]
fn initialize_result_advertises_only_tools() {
    let (_, resp) = parts(INITIALIZE);
    let r: Response = serde_json::from_value(resp).unwrap();
    let result: InitializeResult = serde_json::from_value(r.result().unwrap().clone()).unwrap();
    assert_eq!(result.protocol_version, "2025-06-18");
    assert_eq!(result.server_info.name, "secure-filesystem-server");
    assert!(result.capabilities.has_tools(), "server advertised tools");
    assert!(result.capabilities.prompts.is_none(), "no prompts advertised");
    assert!(result.capabilities.resources.is_none(), "no resources advertised");
}

// --- Step 3: tools/list + tools/call ---------------------------------------

#[test]
fn tools_list_parses_all_fourteen_tools() {
    let (_, resp) = parts(TOOLS_LIST);
    let r: Response = serde_json::from_value(resp).unwrap();
    let list: ToolsListResult = serde_json::from_value(r.result().unwrap().clone()).unwrap();
    assert_eq!(list.tools.len(), 14, "real filesystem server exposes 14 tools");

    let read_file = list.tools.iter().find(|t| t.name == "read_file").expect("read_file present");
    assert_eq!(read_file.title.as_deref(), Some("Read File (Deprecated)"));

    let read_text = list.tools.iter().find(|t| t.name == "read_text_file").unwrap();
    let ann = read_text.annotations.as_ref().expect("read_text_file has annotations");
    assert_eq!(ann.read_only_hint, Some(true));
    assert_eq!(ann.open_world_hint, Some(false));
}

#[test]
fn tools_call_success_yields_text_and_structured_content() {
    let (_, resp) = parts(TOOLS_CALL);
    let r: Response = serde_json::from_value(resp).unwrap();
    let result: CallToolResult = serde_json::from_value(r.result().unwrap().clone()).unwrap();
    assert!(!result.is_error(), "a successful read is not an error");
    assert!(result.structured_content.is_some(), "server sent structuredContent");
    match &result.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("Hello from QuantaMind")),
        other => panic!("expected a text block, got {other:?}"),
    }
}

#[test]
fn tool_inband_error_sets_is_error_true() {
    let (_, resp) = parts(ERR_INBAND);
    let r: Response = serde_json::from_value(resp).unwrap();
    // Reaches us on the SUCCESS protocol channel...
    let result: CallToolResult = serde_json::from_value(r.result().unwrap().clone()).unwrap();
    // ...but the tool itself failed, flagged in-band.
    assert!(result.is_error(), "isError:true must surface as a tool error");
    match &result.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("Access denied")),
        other => panic!("expected a text block, got {other:?}"),
    }
}

// --- Unknown-field / unknown-type tolerance (the "dropped all tools" bug) ---

#[test]
fn unknown_tool_fields_are_tolerated() {
    // A future server tacks on fields we don't model. Must still parse.
    let json = serde_json::json!({
        "name": "future_tool",
        "description": "does something",
        "inputSchema": {"type": "object", "properties": {}},
        "execution": {"taskSupport": "optional"},
        "someBrandNewField": {"nested": true}
    });
    let t: Tool = serde_json::from_value(json).unwrap();
    assert_eq!(t.name, "future_tool");
}

#[test]
fn unknown_content_block_type_falls_to_other() {
    let json = serde_json::json!({"type": "hologram", "payload": "…"});
    let b: ContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(b, ContentBlock::Other);
}

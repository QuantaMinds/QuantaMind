use super::*;
use crate::inference::mcp::oracle_schema::CallCheck;
use crate::mcp::wire::Response;
use serde_json::json;

fn resp(v: serde_json::Value) -> Response {
    serde_json::from_value(v).unwrap()
}

#[test]
fn wire_outcome_classifies_the_three_shapes() {
    let ok = resp(json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"hi"}]}}));
    assert_eq!(wire_outcome(&ok), WireOutcome::Ok { is_error: false });

    let tool_err = resp(json!({"jsonrpc":"2.0","id":1,
        "result":{"content":[{"type":"text","text":"denied"}],"isError":true}}));
    assert_eq!(wire_outcome(&tool_err), WireOutcome::Ok { is_error: true });

    let proto = resp(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}));
    assert_eq!(wire_outcome(&proto), WireOutcome::ProtocolError { code: -32601 });
}

#[test]
fn a_bad_call_is_always_the_models_fault() {
    assert_eq!(attribute(&CallCheck::UnknownTool, &WireOutcome::Ok { is_error: false }), Attribution::Model);
    assert_eq!(
        attribute(&CallCheck::Invalid(vec!["x".into()]), &WireOutcome::Transport),
        Attribution::Model
    );
}

#[test]
fn a_valid_call_is_attributed_by_the_wire_outcome() {
    let v = CallCheck::Valid;
    assert_eq!(attribute(&v, &WireOutcome::Ok { is_error: false }), Attribution::Success);
    // in-band tool error → server/tool
    assert_eq!(attribute(&v, &WireOutcome::Ok { is_error: true }), Attribution::Server);
    // unknown method → config; invalid params (after our check passed) → config
    assert_eq!(attribute(&v, &WireOutcome::ProtocolError { code: -32601 }), Attribution::Config);
    assert_eq!(attribute(&v, &WireOutcome::ProtocolError { code: -32602 }), Attribution::Config);
    // internal / server-defined → server
    assert_eq!(attribute(&v, &WireOutcome::ProtocolError { code: -32603 }), Attribution::Server);
    assert_eq!(attribute(&v, &WireOutcome::ProtocolError { code: -32000 }), Attribution::Server);
    // connection died → server
    assert_eq!(attribute(&v, &WireOutcome::Transport), Attribution::Server);
}

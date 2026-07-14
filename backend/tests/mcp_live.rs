//! Phase 2 LIVE verification (CLAUDE.md rule 6). Ignored by default — it spawns
//! a real MCP server via `npx` (needs network on first run). Run explicitly:
//!
//!   cargo test --test mcp_live -- --ignored --nocapture
//!
//! Asserts a real round-trip and BOTH error channels against
//! @modelcontextprotocol/server-filesystem.

use quantamind_lib::mcp::client::McpClient;
use quantamind_lib::mcp::wire::ContentBlock;

fn fs_server_args(dir: &str) -> [String; 3] {
    ["-y".into(), "@modelcontextprotocol/server-filesystem".into(), dir.into()]
}

#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn filesystem_server_round_trip_and_both_error_channels() {
    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&notes, "hello from the live test\n").unwrap();
    let dir = tmp.path().to_str().unwrap().to_string();

    let client = McpClient::connect("npx", &fs_server_args(&dir), "quantamind-live-test", "0.0.0")
        .await
        .expect("connect + initialize");

    println!("server = {:?}, protocol = {}", client.server_info(), client.protocol_version());
    assert!(client.capabilities().has_tools(), "filesystem server advertises tools");

    // tools/list — real schemas.
    let tools = client.list_tools().await.expect("tools/list");
    let names: Vec<_> = tools.tools.iter().map(|t| t.name.clone()).collect();
    println!("tools = {names:?}");
    assert!(names.iter().any(|n| n == "read_text_file"), "read_text_file present");

    // tools/call success — real content read from the sandbox.
    let ok = client
        .call_tool("read_text_file", serde_json::json!({ "path": notes.to_str().unwrap() }))
        .await
        .expect("tools/call read_text_file");
    assert!(!ok.is_error(), "a valid read is not an error");
    match &ok.content[0] {
        ContentBlock::Text { text } => assert!(text.contains("hello from the live test")),
        other => panic!("expected text content, got {other:?}"),
    }

    // Channel 1 — in-band tool error: reading outside the allowed dir.
    let denied = client
        .call_tool("read_text_file", serde_json::json!({ "path": "/etc/hosts" }))
        .await
        .expect("protocol success even though the tool fails");
    assert!(denied.is_error(), "outside-sandbox read is an in-band tool error (isError:true)");

    // Channel 2 — protocol error: an unknown JSON-RPC METHOD → -32601.
    let unknown = client.transport().request("this/does_not_exist", None).await.unwrap();
    assert_eq!(unknown.result().unwrap_err().code, -32601, "unknown method is -32601");

    client.kill();
}

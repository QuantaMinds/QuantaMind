//! Phase 2 LIVE verification (CLAUDE.md rule 6). Ignored by default — it spawns
//! a real MCP server via `npx` (needs network on first run). Run explicitly:
//!
//!   cargo test --test mcp_live -- --ignored --nocapture
//!
//! Asserts a real round-trip and BOTH error channels against
//! @modelcontextprotocol/server-filesystem.

use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::mcp::bridge::{execute_call, single_turn};
use quantamind_lib::inference::ollama::ollama_chat::NativeToolCall;
use quantamind_lib::mcp::client::McpClient;
use quantamind_lib::mcp::wire::ContentBlock;
use quantamind_lib::persistence::mcp::servers::McpServerConfig;

async fn connect_fs(dir: &str) -> McpClient {
    McpClient::connect(
        "npx",
        &["-y".into(), "@modelcontextprotocol/server-filesystem".into(), dir.into()],
        "quantamind-live-test",
        "0.0.0",
    )
    .await
    .expect("connect fs server")
}

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

/// Phase 4: the `probe` flow — build spawn args from a registry config (args +
/// canonical roots), connect with env, list tools. Exercises `canonical_roots`
/// and `connect_with_env` against the real server.
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn probe_flow_from_a_registry_config_lists_tools() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
    let cfg = McpServerConfig {
        id: "filesystem".into(),
        command: "npx".into(),
        args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()],
        env_keys: vec![],
        roots: vec![tmp.path().to_str().unwrap().into()],
        enabled: true,
    };
    let mut args = cfg.args.clone();
    for r in cfg.canonical_roots().unwrap() {
        args.push(r.to_string_lossy().into_owned());
    }
    let client =
        McpClient::connect_with_env(&cfg.command, &args, &[], "quantamind", "0.0.0", std::time::Duration::from_secs(30))
            .await
            .expect("connect from config");
    let tools = client.list_tools().await.expect("tools/list");
    assert_eq!(tools.tools.len(), 14, "filesystem server exposes 14 tools");
    client.kill();
}

/// Phase 5: the bridge executes a (stubbed) model call against a REAL server —
/// proves execute+inject without needing an LLM. Both channels checked.
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn bridge_executes_a_stubbed_call_against_the_real_server() {
    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&notes, "hello from the live test\n").unwrap();
    let client = connect_fs(tmp.path().to_str().unwrap()).await;

    // A well-formed call → real content, not an error.
    let call = NativeToolCall {
        name: "read_text_file".into(),
        args: serde_json::json!({ "path": notes.to_str().unwrap() }),
    };
    let exec = execute_call(&client, &call).await.unwrap();
    assert!(!exec.is_error);
    assert!(exec.text.contains("hello from the live test"), "got: {}", exec.text);

    // A sandbox escape → in-band tool error surfaced on the execution.
    let escape = NativeToolCall {
        name: "read_text_file".into(),
        args: serde_json::json!({ "path": "/etc/hosts" }),
    };
    let exec = execute_call(&client, &escape).await.unwrap();
    assert!(exec.is_error, "outside-sandbox read is an error");
    client.kill();
}

/// Phase 5: a REAL model drives a REAL tool. Gated on env so the default
/// `--ignored` run doesn't require a model:
///   MCP_MODEL=qwen2.5:1.5b [MCP_BACKEND=ollama|llama] [MCP_ENDPOINT=...] \
///     cargo test --test mcp_live -- --ignored bridge_single_turn --nocapture
#[tokio::test]
#[ignore = "requires a running Ollama/llama-server model"]
async fn bridge_single_turn_reads_a_file_via_a_real_model() {
    let Ok(model) = std::env::var("MCP_MODEL") else {
        eprintln!("SKIP: set MCP_MODEL to run the real-model bridge test");
        return;
    };
    let backend = match std::env::var("MCP_BACKEND").as_deref() {
        Ok("llama") => BackendKind::LlamaCpp,
        _ => BackendKind::Ollama,
    };
    let endpoint = std::env::var("MCP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let tmp = tempfile::tempdir().unwrap();
    let notes = tmp.path().join("notes.txt");
    std::fs::write(&notes, "the secret word is bluebird\n").unwrap();
    let client = connect_fs(tmp.path().to_str().unwrap()).await;
    let tools = client.list_tools().await.unwrap().tools;

    let user = format!(
        "Use the read_text_file tool to read the file at {} and report its contents.",
        notes.to_str().unwrap()
    );
    let turn = single_turn(
        backend,
        &endpoint,
        &model,
        "You are a tool-using assistant. Call the provided tools to answer.",
        &user,
        &tools,
        None,
        &client,
    )
    .await
    .expect("single_turn");

    eprintln!(
        "[{backend:?} {model}] calls={:?} warning={:?}",
        turn.calls.iter().map(|c| &c.name).collect::<Vec<_>>(),
        turn.warning
    );
    assert!(
        turn.executions.iter().any(|e| e.text.contains("bluebird")),
        "the model should have read the file via read_text_file; calls={:?}",
        turn.calls
    );
    client.kill();
}

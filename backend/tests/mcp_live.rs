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

/// Phase 8: the world-manager end to end — seed a world, drive REAL tools, grade
/// the WORLD's end-state (not words), tear down. No LLM needed (stubbed calls).
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn world_seed_execute_grade_endstate_and_teardown() {
    use quantamind_lib::inference::eval::mcp::world::{FsSeed, McpWorld};

    let seed = FsSeed::from([("old.log", "stale"), ("keep.txt", "keep me")]);
    let root_path;
    {
        let world = McpWorld::filesystem(&seed).await.expect("seed + scoped server");
        root_path = world.root().to_path_buf();
        assert!(root_path.join("old.log").exists(), "seed written");

        // Drive a REAL write via the server, then confirm the WORLD changed.
        let new_file = root_path.join("added.txt");
        let ex = world
            .execute(&NativeToolCall {
                name: "write_file".into(),
                args: serde_json::json!({ "path": new_file.to_str().unwrap(), "content": "hello world" }),
            })
            .await
            .expect("write_file");
        assert!(!ex.is_error, "write should succeed inside the sandbox; got: {}", ex.text);
        // The oracle grades the WORLD, not the model's claim:
        assert!(new_file.exists(), "the file actually exists on disk");
        assert_eq!(std::fs::read_to_string(&new_file).unwrap(), "hello world");

        // Isolation: a write OUTSIDE the sandbox is refused by the scoped server.
        let escape = world
            .execute(&NativeToolCall {
                name: "write_file".into(),
                args: serde_json::json!({ "path": "/tmp/qm-escape-should-fail.txt", "content": "x" }),
            })
            .await
            .expect("protocol ok");
        assert!(escape.is_error, "outside-sandbox write is refused");
    } // world dropped → server killed, dir removed

    assert!(!root_path.exists(), "teardown removed the per-run world");
}

/// Phase 8: fresh-per-run — two worlds get distinct directories (pass^k needs a
/// byte-identical reset, i.e. a brand-new world each run).
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn each_world_run_gets_a_distinct_fresh_dir() {
    use quantamind_lib::inference::eval::mcp::world::{FsSeed, McpWorld};
    let seed = FsSeed::default();
    let w1 = McpWorld::filesystem(&seed).await.unwrap();
    let w2 = McpWorld::filesystem(&seed).await.unwrap();
    assert_ne!(w1.root(), w2.root(), "each run is a fresh, distinct world");
}

/// Phase 9: the approval gate GOVERNS real side effects. A scripted "model"
/// attempts one write against a real seeded world: DENY → world unchanged;
/// APPROVE → world mutated for real. Deterministic (no model nondeterminism).
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn approval_gate_controls_real_world_mutation() {
    use quantamind_lib::inference::eval::mcp::world::{FsSeed, McpWorld};
    use quantamind_lib::inference::mcp::agent::{run_loop, McpExecutor, TurnDriver, TurnOutput};
    use quantamind_lib::inference::mcp::gate::Decision;

    // A one-shot "model": turn 1 asks to write `path`, then yields.
    struct OneShotWrite {
        path: String,
        fired: bool,
    }
    impl TurnDriver for OneShotWrite {
        async fn turn(&mut self, _transcript: &str) -> quantamind_lib::errors::AppResult<TurnOutput> {
            if self.fired {
                return Ok(TurnOutput { text: "done".into(), calls: vec![] });
            }
            self.fired = true;
            Ok(TurnOutput {
                text: String::new(),
                calls: vec![NativeToolCall {
                    name: "write_file".into(),
                    args: serde_json::json!({ "path": self.path, "content": "written" }),
                }],
            })
        }
    }

    let seed = FsSeed::from([("keep.txt", "keep")]);

    // DENY → the write never reaches the server; the world is unchanged.
    {
        let world = McpWorld::filesystem(&seed).await.unwrap();
        let target = world.root().join("new.txt");
        let mut driver = OneShotWrite { path: target.to_str().unwrap().into(), fired: false };
        let exec = McpExecutor::new(world.client());
        let out = run_loop(&mut driver, &exec, |_| Decision::Deny, 3).await.unwrap();
        assert_eq!(out.denied, 1);
        assert!(!target.exists(), "DENY → the real world is NOT mutated");
    }

    // APPROVE → the write executes; the file really exists.
    {
        let world = McpWorld::filesystem(&seed).await.unwrap();
        let target = world.root().join("new.txt");
        let mut driver = OneShotWrite { path: target.to_str().unwrap().into(), fired: false };
        let exec = McpExecutor::new(world.client());
        let _ = run_loop(&mut driver, &exec, |_| Decision::Approve, 3).await.unwrap();
        assert!(target.exists(), "APPROVE → the real world IS mutated");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "written");
    }
}

/// Phase 10: pass^k end-state scoring + FAKE-DONE detection. Task: "create
/// result.txt containing DONE" against a controlled world. An honest model that
/// actually writes → ready (k/k). A model that only SAYS it did → NOT ready
/// (0/k), because the oracle grades the world, not the words.
#[tokio::test]
#[ignore = "spawns a real MCP server via npx"]
async fn passk_scoring_catches_fake_done() {
    use quantamind_lib::inference::eval::mcp::oracle_fs::FsOracle;
    use quantamind_lib::inference::eval::mcp::score::{score_fs_task, McpTask};
    use quantamind_lib::inference::eval::mcp::world::FsSeed;
    use quantamind_lib::inference::mcp::agent::{TurnDriver, TurnOutput};

    // Honest: turn 1 writes result.txt, then yields.
    struct Honest { path: String, fired: bool }
    impl TurnDriver for Honest {
        async fn turn(&mut self, _t: &str) -> quantamind_lib::errors::AppResult<TurnOutput> {
            if self.fired { return Ok(TurnOutput { text: "done".into(), calls: vec![] }); }
            self.fired = true;
            Ok(TurnOutput { text: String::new(), calls: vec![NativeToolCall {
                name: "write_file".into(),
                args: serde_json::json!({ "path": self.path, "content": "the answer is DONE" }),
            }] })
        }
    }
    // Fake-done: claims success, calls nothing.
    struct FakeDone;
    impl TurnDriver for FakeDone {
        async fn turn(&mut self, _t: &str) -> quantamind_lib::errors::AppResult<TurnOutput> {
            Ok(TurnOutput { text: "Done! I created result.txt with DONE.".into(), calls: vec![] })
        }
    }

    let task = McpTask {
        instruction: "Create result.txt containing DONE".into(),
        seed: FsSeed::default(),
        oracle: FsOracle {
            assert_present: vec!["result.txt".into()],
            assert_content: vec![("result.txt".into(), "DONE".into())],
            ..Default::default()
        },
    };

    let honest = score_fs_task(&task, |root, _tools| Honest {
        path: root.join("result.txt").to_str().unwrap().into(),
        fired: false,
    }, 3, 4).await.unwrap();
    eprintln!("honest: {}/{} ready={}", honest.passes, honest.k, honest.is_ready());
    assert!(honest.is_ready(), "an honest model that really writes is ready (k/k)");

    let fake = score_fs_task(&task, |_root, _tools| FakeDone, 3, 4).await.unwrap();
    eprintln!("fake-done: {}/{} ready={} failures[0]={:?}", fake.passes, fake.k, fake.is_ready(), fake.failures.first());
    assert!(!fake.is_ready(), "a model that only SAYS done is NOT ready");
    assert_eq!(fake.passes, 0, "fake-done never actually creates the file");
    assert!(fake.failures[0].iter().any(|f| f.contains("result.txt")));
}

/// Phase 11: a second world type (sqlite) proves the world/oracle abstraction
/// generalizes. Seed users(Bob); task "insert Alice"; grade the DB end-state via
/// an independent SELECT. Honest → ready; fake-done → not ready.
#[tokio::test]
#[ignore = "spawns a real sqlite MCP server via npx"]
async fn db_world_scores_insert_and_catches_fake_done() {
    use quantamind_lib::inference::eval::mcp::oracle_db::DbOracle;
    use quantamind_lib::inference::eval::mcp::score::{score_db_task, DbTask};
    use quantamind_lib::inference::eval::mcp::world::DbSeed;
    use quantamind_lib::inference::mcp::agent::{TurnDriver, TurnOutput};

    struct InsertAlice { fired: bool }
    impl TurnDriver for InsertAlice {
        async fn turn(&mut self, _t: &str) -> quantamind_lib::errors::AppResult<TurnOutput> {
            if self.fired { return Ok(TurnOutput { text: "done".into(), calls: vec![] }); }
            self.fired = true;
            Ok(TurnOutput { text: String::new(), calls: vec![NativeToolCall {
                name: "write_query".into(),
                args: serde_json::json!({ "query": "INSERT INTO users(name) VALUES('Alice')" }),
            }] })
        }
    }
    struct FakeDone;
    impl TurnDriver for FakeDone {
        async fn turn(&mut self, _t: &str) -> quantamind_lib::errors::AppResult<TurnOutput> {
            Ok(TurnOutput { text: "Done! Added Alice.".into(), calls: vec![] })
        }
    }

    let task = DbTask {
        instruction: "Insert a row for Alice into users".into(),
        seed: DbSeed::new("CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO users(name) VALUES('Bob');"),
        oracle: DbOracle {
            assert_eq: vec![("SELECT COUNT(*) FROM users WHERE name='Alice';".into(), "1".into())],
            ..Default::default()
        },
    };

    let honest = score_db_task(&task, |_db, _tools| InsertAlice { fired: false }, 3, 4).await.unwrap();
    eprintln!("db honest: {}/{} ready={}", honest.passes, honest.k, honest.is_ready());
    assert!(honest.is_ready(), "a model that really inserts Alice is ready (k/k)");

    let fake = score_db_task(&task, |_db, _tools| FakeDone, 3, 4).await.unwrap();
    eprintln!("db fake-done: {}/{} ready={} failures[0]={:?}", fake.passes, fake.k, fake.is_ready(), fake.failures.first());
    assert!(!fake.is_ready(), "a model that only SAYS it inserted is NOT ready");
    assert_eq!(fake.passes, 0);
}

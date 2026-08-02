//! Phase 7 LIVE-backend suite — drives the real the server / llama.cpp / the remote server servers
//! and real models through the app's own inference + eval + readiness code. Every
//! test is `#[ignore]` (it needs live servers + GB-scale models, so it never runs
//! in the normal `cargo test`). Run explicitly:
//!
//!   cargo test --test phase7_live_backends -- --ignored --nocapture
//!   cargo test --test phase7_live_backends live_the server -- --ignored --nocapture
//!
//! Model paths default to this machine's layout and are overridable by env var.

use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::backend::endpoint;
use quantamind_lib::inference::eval::agentic::report::{FailureTracker, TopError};
use quantamind_lib::inference::eval::batch::{AggAgentic, BatchColumn};
use quantamind_lib::inference::eval::readiness::inputs::verdict_for;
use quantamind_lib::inference::eval::readiness::profile::ReadinessProfile;
use quantamind_lib::inference::eval::readiness::recommend;
use quantamind_lib::inference::eval::readiness::types::{ModelVerdict, Readiness};
use quantamind_lib::inference::eval::toolcall::eval::run_eval;
use quantamind_lib::inference::eval::toolcall::tasks::{Call, Expected, ToolSchema, ToolTask};
use quantamind_lib::inference::the server::the server_chat::chat_with_tools;
use quantamind_lib::inference::the server::the server_show::probe_supports_tools;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

// ── env-overridable model handles (defaults match this machine) ──────────────

fn home() -> String {
    std::env::var("HOME").unwrap()
}
fn the server_tool_model() -> String {
    std::env::var("QM_LIVE_MODEL").unwrap_or_else(|_| "llama3.2:3b".into())
}
fn notools_model() -> String {
    std::env::var("QM_LIVE_NOTOOLS").unwrap_or_else(|_| "gamma-2b-instruct-ft-awesome-chatgpt-prompts:q2_k".into())
}
fn llama_gguf() -> String {
    std::env::var("QM_LLAMA_GGUF").unwrap_or_else(|_| format!("{}/.quantamind/gguf/llama-3.2-1b-instruct_q8_0.gguf", home()))
}
fn remote_model() -> String {
    std::env::var("QM_the remote server_MODEL").unwrap_or_else(|_| format!("{}/.quantamind/remote/remote-community_Llama-3.2-1B-Instruct-4bit", home()))
}

fn weather_tools() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the current weather for a city",
            "parameters": { "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }
        }
    }])
}

fn weather_task() -> ToolTask {
    ToolTask {
        id: "weather".into(),
        category: "single".into(),
        prompt: "What is the weather in Paris? Call the tool.".into(),
        tools: vec![ToolSchema {
            name: "get_weather".into(),
            description: "Get the current weather for a city".into(),
            parameters: json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }),
        }],
        expected: Expected::Call(Call { name: "get_weather".into(), args: json!({ "city": "Paris" }) }),
        agentic: None,
    }
}

fn profile(min_pass_k: f64) -> ReadinessProfile {
    ReadinessProfile {
        id: "live".into(),
        name: "Live coding agent".into(),
        min_pass_k,
        max_avg_steps: None,
        max_ms_per_step: None,
        min_context_tokens: None,
        forbid_infinite_loop: true,
        forbid_hallucinated_completion: true,
        require_full_vram: false,
        require_native_fc: false,
    }
}

fn called_weather_paris(name: &str, args: &Value) -> bool {
    name == "get_weather"
        && args
            .get("city")
            .and_then(|c| c.as_str())
            .map(|c| c.to_lowercase().contains("paris"))
            .unwrap_or(false)
}

// ── S3 — the server native function-calling against a REAL tool-capable model ─────

#[tokio::test]
#[ignore]
async fn live_the server_native_tool_call_and_capability_probe() {
    let model = the server_tool_model();
    let res = chat_with_tools(endpoint::LLAMA_SERVER, &model, "You are a helpful assistant.", "What is the weather in Paris? Call the tool.", &weather_tools(), None)
        .await
        .expect("native /api/chat call failed");
    println!("[the server native FC] model={model} tool_calls={:?} content={:?}", res.tool_calls, res.content);
    assert!(!res.tool_calls.is_empty(), "expected a native tool_call from {model}");
    let tc = &res.tool_calls[0];
    assert!(called_weather_paris(&tc.name, &tc.args), "expected get_weather(Paris), got {}({})", tc.name, tc.args);

    // Capability probe: tool-capable model true, non-tool model false.
    let tools_ok = probe_supports_tools(endpoint::LLAMA_SERVER, &model).await;
    let notools = probe_supports_tools(endpoint::LLAMA_SERVER, &notools_model()).await;
    println!("[capability probe] {model}={tools_ok}  {}={notools}", notools_model());
    assert!(tools_ok, "{model} should report the tools capability");
    assert!(!notools, "{} should NOT report tools (N/A path)", notools_model());
}

// ── S1 — the server single-turn tool-calling eval (real scoring) ─────────────────

#[tokio::test]
#[ignore]
async fn live_the server_toolcall_eval_scores_a_real_model() {
    let model = the server_tool_model();
    let report = run_eval(BackendKind::LlamaCpp, endpoint::LLAMA_SERVER, &model, &[weather_task()]).await.expect("run_eval failed");
    println!(
        "[the server toolcall eval] model={model} n={} parse={:?} select={:?} arg={:?} composite={:?}",
        report.n, report.parse_rate, report.tool_selection_acc, report.arg_acc, report.composite
    );
    assert_eq!(report.n, 1);
    assert!(report.composite.is_some(), "a live run must yield a real composite score");
}

// ── S3→S1→S5 — real native pass^k feeds the REAL verdict + recommender ────────

#[tokio::test]
#[ignore]
async fn live_the server_native_passk_drives_verdict_and_recommender() {
    let model = the server_tool_model();
    let k = std::env::var("QM_K").ok().and_then(|s| s.parse().ok()).unwrap_or(5u32);
    let tools = weather_tools();
    let mut passes = 0u32;
    for i in 0..k {
        match chat_with_tools(endpoint::LLAMA_SERVER, &model, "You are a helpful assistant.", "What is the weather in Paris? Call the tool.", &tools, None).await {
            Ok(r) => {
                let ok = r.tool_calls.first().map(|t| called_weather_paris(&t.name, &t.args)).unwrap_or(false);
                if ok {
                    passes += 1;
                }
                println!("  run {i}: {} -> {:?}", if ok { "PASS" } else { "fail" }, r.tool_calls);
            }
            Err(e) => println!("  run {i}: ERROR {e}"),
        }
    }
    let pass_k = passes as f64 / k as f64;
    println!("[the server native pass^k] model={model} passes={passes}/{k} pass_k={pass_k:.2}");

    // Build a real column from the measured native pass^k, then run the SAME
    // verdict + recommender the GUI/CLI use.
    let native = AggAgentic {
        passes,
        total_runs: k,
        avg_steps: Some(1.0),
        avg_output_tokens_success: Some(20.0),
        schema_resilience: None,
        top_error: TopError::None,
        failures: FailureTracker::default(),
    };
    let col = BatchColumn { model: model.clone(), backend: BackendKind::LlamaCpp, toolcall: None, agentic: None, agentic_native_fc: Some(native), error: None };
    let p = profile(0.80);
    let v = verdict_for(&col, Some(true), false, None, &p);
    println!("[verdict @min_pass_k=0.80] {model} => {:?}  path={:?}  blocking={:?}", v.status, v.path, v.blocking);

    // Rank it next to two synthetic baselines to show the leaderboard logic on a real row.
    use quantamind_lib::inference::eval::readiness::types::{AgentPath, ReadinessVerdict};
    let mk = |m: &str, s: Readiness, eff: f64| ModelVerdict {
        model: m.into(),
        backend: BackendKind::LlamaCpp,
        verdict: ReadinessVerdict { status: s, blocking: vec![], conditions: vec![], path: AgentPath::NativeFc },
        memory: None,
        avg_steps: Some(1.0),
        effort: Some(eff),
        pass_k: None,
        quantization: None,
        cliff_tokens: None,
    };
    let mut board = vec![
        ModelVerdict { model: model.clone(), backend: BackendKind::LlamaCpp, verdict: v.clone(), memory: None, avg_steps: Some(1.0), effort: Some(20.0), pass_k: None, quantization: None, cliff_tokens: None },
        mk("synthetic-notready", Readiness::NotReady, 10.0),
        mk("synthetic-ready-costly", Readiness::Ready, 999.0),
    ];
    recommend::rank(&mut board);
    let order: Vec<&str> = board.iter().map(|m| m.model.as_str()).collect();
    println!("[recommender leaderboard] {order:?}  -> recommended = {}", recommend::recommendation(&board).unwrap().model);

    assert!(passes >= 1, "a tool-capable model should pass at least once in {k} tries");
}

// ── DIAGNOSTIC — per-task Pass/Fail accuracy of the prompt-based scorer ───────
// Runs the bundled CURATED tool-call tasks against a live model and prints, per
// task: the verdict booleans + computed pass/fail + the model's RAW output + the
// expected, so a false Fail (extraction / args strictness / model format) is
// visible. Run: cargo test --test phase7_live_backends diagnose_scoring -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn diagnose_prompt_based_scoring_accuracy() {
    use quantamind_lib::inference::eval::toolcall::eval::run_eval_traced;
    use quantamind_lib::inference::eval::toolcall::tasks::tasks;

    let model = the server_tool_model();
    let all = tasks();
    let single: Vec<_> = all.into_iter().filter(|t| t.category != "agentic").collect();
    println!("=== prompt-based scoring of {} single-turn tasks on {model} ===", single.len());
    let mut pass = 0;
    for t in &single {
        let (_r, traces) =
            run_eval_traced(BackendKind::LlamaCpp, endpoint::LLAMA_SERVER, &model, std::slice::from_ref(t), None).await.unwrap();
        let v = &traces[0].trace.verdict;
        let passed = match v.abstain_correct {
            Some(ok) => ok,
            None => v.parsed && v.tool_match && v.args_match,
        };
        if passed {
            pass += 1;
        }
        println!(
            "\n[{}] cat={} => {}  (parsed={} tool={} args={} abstain={:?})",
            t.id,
            t.category,
            if passed { "PASS" } else { "FAIL" },
            v.parsed,
            v.tool_match,
            v.args_match,
            v.abstain_correct
        );
        println!("    expected: {:?}", t.expected);
        let raw = traces[0].trace.raw_output.replace('\n', " ");
        println!("    raw[0..200]: {}", raw.chars().take(200).collect::<String>());
    }
    println!("\n=== {pass}/{} passed ===", single.len());
}

// ── S1+S2+S4+S5+S6 — a REAL agentic multi-model batch → verdicts → recommender ─
// Runs the app's real engine (run_batch_resumable) over two live the server models
// with the REAL the server VRAM-isolation gate and a real job log, then walks the
// readiness scenarios off the resulting BatchReport.

use quantamind_lib::inference::eval::agentic::sandbox::{EndStateRule, MockResponse, TaskCheckpoint};
use quantamind_lib::inference::eval::agentic::spec::AgenticSpec;
use quantamind_lib::inference::eval::batch::{run_batch_resumable, BatchSink, the serverVramGate, TaskOutcome};
use quantamind_lib::inference::eval::agentic::model_turn::BackendTurn;
use quantamind_lib::inference::eval::readiness::inputs::assess_report;
use quantamind_lib::inference::eval::toolcall::matrix::ModelTarget;
use quantamind_lib::persistence::jobs::queue::{self, RunConfig};
use std::sync::Arc;

struct SilentSink;
impl BatchSink for SilentSink {
    fn task_started(&self, _m: &str, _t: &str, _i: usize, _n: usize, _c: &str) {}
    fn agentic_turn(&self, _m: &str, _t: &str, _s: &quantamind_lib::inference::eval::agentic::step::TrajectoryStep) {}
    fn task_done(&self, _m: &str, _t: &str, _o: &TaskOutcome) {}
}

fn weather_agentic_task() -> ToolTask {
    let mut t = weather_task();
    t.id = "weather_agentic".into();
    t.category = "agentic".into();
    t.prompt = "Find the current weather in Paris using the available tool, then report it.".into();
    t.agentic = Some(AgenticSpec {
        mocks: vec![MockResponse {
            call: Call { name: "get_weather".into(), args: json!({ "city": "Paris" }) },
            response: "18°C and sunny in Paris.".into(),
        }],
        end_state: EndStateRule::RequireSequence(vec![TaskCheckpoint { tool: "get_weather".into(), args: json!({ "city": "Paris" }) }]),
        k: Some(2),
        max_steps: Some(6),
        faults: vec![],
        max_recovery: None,
    });
    t
}

#[tokio::test]
#[ignore]
async fn live_full_readiness_walk_real_agentic_batch() {
    let strong = the server_tool_model(); // llama3.2:3b — should navigate the sandbox
    let weak = std::env::var("QM_LIVE_WEAK").unwrap_or_else(|_| "llama-3.2-1b-instruct_q8_0:latest".into());
    let targets = vec![
        ModelTarget { model: strong.clone(), backend: BackendKind::LlamaCpp },
        ModelTarget { model: weak.clone(), backend: BackendKind::LlamaCpp },
    ];
    let tasks = vec![weather_agentic_task()];

    // Real job log (S4) + the real VRAM-isolation gate between the two models.
    let dir = std::env::temp_dir().join("qm_live_jobs");
    let _ = std::fs::create_dir_all(&dir);
    let path = queue::run_path(&dir, "live-collection");
    let cfg = RunConfig { collection_id: "live-collection".into(), targets: targets.clone(), tasks: tasks.clone(), k: Some(2), max_steps: Some(6), params: None, keep_alive: None, native: false };
    queue::create(&path, &cfg).unwrap();
    let rec_path = path.clone();
    let record = move |u: &_| {
        let _ = queue::append(&rec_path, u);
    };

    println!("[batch] running REAL agentic batch over {strong} + {weak} with the serverVramGate (VRAM isolation between models)…");
    let sink: Arc<dyn BatchSink> = Arc::new(SilentSink);
    let report = run_batch_resumable(
        "live-collection",
        &targets,
        &tasks,
        CancellationToken::new(),
        sink,
        move |t: &ModelTarget| BackendTurn { backend: t.backend, endpoint: endpoint::LLAMA_SERVER.to_string(), model: t.model.clone(), cancel: CancellationToken::new(), options: None, keep_alive: None },
        &[],
        &record,
        &the serverVramGate,
    )
    .await
    .expect("batch failed");

    println!("\n=== REAL BatchReport ===");
    for c in &report.columns {
        let a = c.agentic.as_ref();
        println!(
            "  {:<40} agentic={}  err={:?}",
            c.model,
            a.map(|x| format!("pass {}/{} (pass^k={:.2}) loops={} avg_steps={:?}", x.passes, x.total_runs, x.passes as f64 / x.total_runs.max(1) as f64, x.failures.infinite_loop_hits, x.avg_steps)).unwrap_or_else(|| "none".into()),
            c.error
        );
    }

    // ── S1: verdicts + interpolated reasons under a Coding-agent-like profile ──
    println!("\n=== S1: verdicts @ min_pass_k=0.60 (Prompt-Based path) ===");
    let v_lenient = assess_report(&report, &profile(0.60));
    for v in &v_lenient {
        println!(
            "  {:<40} {:?}  pass^k={:?} steps={:?} effort={:?}  path={:?}  blocking={:?}",
            v.model, v.verdict.status, v.pass_k, v.avg_steps, v.effort, v.verdict.path, v.verdict.blocking
        );
    }
    // Real metrics must be populated for a model that produced agentic data.
    let strong_v = v_lenient.iter().find(|v| v.model == strong).unwrap();
    assert!(strong_v.pass_k.is_some(), "real pass^k must flow into the verdict");

    // ── S1: raise the bar → a previously-passing model flips deterministically ──
    println!("\n=== S1: same data @ min_pass_k=0.99 (stricter) ===");
    let v_strict = assess_report(&report, &profile(0.99));
    for v in &v_strict {
        println!("  {:<40} {:?}  blocking={:?}", v.model, v.verdict.status, v.verdict.blocking);
    }
    let strong_lenient = v_lenient.iter().find(|v| v.model == strong).map(|v| v.verdict.status);
    let strong_strict = v_strict.iter().find(|v| v.model == strong).map(|v| v.verdict.status);
    println!("  flip check: {strong} {:?} -> {:?}", strong_lenient, strong_strict);

    // ── S5: rank into the recommendation leaderboard ──
    let mut board = assess_report(&report, &profile(0.60));
    recommend::rank(&mut board);
    println!("\n=== S5: recommendation leaderboard ===");
    for (i, v) in board.iter().enumerate() {
        println!("  #{} {:<40} {:?} effort={:?}", i + 1, v.model, v.verdict.status, v.effort);
    }
    let top = recommend::recommendation(&board).unwrap();
    let banner = match top.verdict.status {
        Readiness::Ready => format!("Recommended for Coding agent: {} (Ready)", top.model),
        Readiness::Conditional => format!("Best available: {} (Conditional)", top.model),
        Readiness::NotReady => format!("No model is ready — closest: {} (NotReady — {})", top.model, top.verdict.blocking.first().cloned().unwrap_or_default()),
    };
    println!("  BANNER: {banner}");

    // ── S4: the real job log on disk (Header + per-(model,task) units) ──
    let (loaded_cfg, units) = queue::load(&path).unwrap().expect("job log present");
    println!("\n=== S4: job log {} ===", path.display());
    println!("  header.collection={} native={}", loaded_cfg.collection_id, loaded_cfg.native);
    for u in &units {
        println!("  unit: model={} task={} is_native={} outcome={}", u.model, u.task_id, u.is_native, match &u.outcome { TaskOutcome::Agentic { .. } => "agentic", TaskOutcome::Single { .. } => "single", TaskOutcome::Error { .. } => "error" });
    }
    // S4 resume: fold the completed units into a partial report (bulk rehydration).
    let partial = quantamind_lib::inference::eval::batch::fold_report("live-collection", &targets, &tasks, &units);
    println!("  fold_report → {} columns rebuilt for instant repaint", partial.columns.iter().filter(|c| c.agentic.is_some()).count());
    queue::delete(&path).unwrap(); // S4 discard
    assert!(queue::load(&path).unwrap().is_none(), "discard removes the log");

    // ── S6: a model with no agentic data is never Ready ──
    let bare = BatchColumn { model: "ghost".into(), backend: BackendKind::LlamaCpp, toolcall: None, agentic: None, agentic_native_fc: None, error: None };
    let bare_report = quantamind_lib::inference::eval::batch::BatchReport { collection_id: "x".into(), num_ctx: None, columns: vec![bare] };
    let bv = assess_report(&bare_report, &profile(0.60));
    println!("\n=== S6: no-agentic-data model => {:?} (must be NotReady) ===", bv[0].verdict.status);
    assert_eq!(bv[0].verdict.status, Readiness::NotReady);

    // Sanity: the strong model should have produced real agentic data.
    let strong_col = report.columns.iter().find(|c| c.model == strong).unwrap();
    assert!(strong_col.agentic.is_some() || strong_col.error.is_some(), "strong model produced neither data nor an error");
    println!("\n[done] real agentic batch + S1/S4/S5/S6 walked against live the server.");
}

// ── S2 — real VRAM fit from REAL model dims, flips at a low cap ───────────────

#[tokio::test]
#[ignore]
async fn live_s2_real_vram_fit_flips_with_cap() {
    use quantamind_lib::commands::models::model_inspect::fetch_dims;
    use quantamind_lib::commands::storage::storage::fetch_installed_with_stats;
    use quantamind_lib::inference::eval::readiness::vram_fit::estimate;

    let model = the server_tool_model();
    let dims = fetch_dims(&model).await.expect("fetch_dims (the server /api/show) failed");
    let weights = fetch_installed_with_stats(endpoint::LLAMA_SERVER)
        .await
        .ok()
        .and_then(|v| v.into_iter().find(|m| m.name == model || m.name == format!("{model}:latest")).map(|m| m.size_bytes))
        .expect("weight size from /api/tags");
    let ctx = 8192u32;
    let roomy = estimate(weights, dims.layers, dims.head_count, dims.head_count_kv, dims.embedding_length, ctx, 24 * 1024u64.pow(3));
    let tight = estimate(weights, dims.layers, dims.head_count, dims.head_count_kv, dims.embedding_length, ctx, 2 * 1024u64.pow(3));
    println!("[S2 real VRAM fit] {model}: weights={:.2}GB kv@{ctx}={:.2}GB", weights as f64 / 1e9, roomy.kv_cache_bytes as f64 / 1e9);
    println!("  cap 24GB → fits={} ; cap 2GB → fits={}", roomy.fits, tight.fits);
    assert!(roomy.fits, "should fit a 24GB cap");
    assert!(!tight.fits, "should NOT fit a 2GB cap");

    // The verdict follows the real fit under a require_full_vram (Coding-agent) profile.
    let mut p = profile(0.60);
    p.require_full_vram = true;
    let col = BatchColumn {
        model: model.clone(),
        backend: BackendKind::LlamaCpp,
        toolcall: None,
        agentic: Some(AggAgentic { passes: 2, total_runs: 2, avg_steps: Some(2.0), avg_output_tokens_success: Some(50.0), schema_resilience: None, top_error: TopError::None, failures: FailureTracker::default() }),
        agentic_native_fc: None,
        error: None,
    };
    let ready = verdict_for(&col, Some(roomy.fits), roomy.pressure, None, &p).status;
    let blocked = verdict_for(&col, Some(tight.fits), false, None, &p).status;
    println!("  verdict @24GB={:?}  @2GB={:?}", ready, blocked);
    assert_eq!(ready, Readiness::Ready);
    assert_eq!(blocked, Readiness::NotReady);
}

// ── llama.cpp — spawn the REAL server (app's own code) + generate ────────────

#[tokio::test]
#[ignore]
async fn live_llamacpp_spawn_and_generate() {
    use quantamind_lib::commands::llama::llama_runtime::{build_spawn_args, kill_server, spawn_server, wait_until_ready, PORT};
    use std::path::PathBuf;

    let gguf = llama_gguf();
    assert!(PathBuf::from(&gguf).exists(), "GGUF not found: {gguf}");
    // The app's dev resolution: <repo>/backend/binaries.
    let bin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    assert!(bin_dir.join("llama-server").exists(), "llama-server not in {bin_dir:?}");

    let mut child = spawn_server(&bin_dir, &build_spawn_args(&gguf, PORT)).expect("spawn llama-server");
    println!("[llama.cpp] spawned pid={} on :{PORT} model={gguf}", child.id());

    let ready = wait_until_ready().await;
    if !ready {
        let _ = kill_server(&mut child);
        panic!("llama-server did not become ready on :{PORT}");
    }
    println!("[llama.cpp] /health OK");

    // llama-server `/completion` is a RAW completion endpoint (no chat template),
    // so use a completion-style prompt the model will continue.
    let mut out = String::new();
    let stats = quantamind_lib::inference::llama::llama::stream_generate(
        endpoint::LLAMA_SERVER,
        "", // single loaded model; name unused
        "The capital of France is",
        None,
        None,
        CancellationToken::new(),
        |t| out.push_str(t),
    )
    .await;
    let _ = kill_server(&mut child); // always reap
    let stats = stats.expect("llama.cpp /completion failed");
    println!("[llama.cpp] output={:?}  tokens={:?}", out.trim(), stats.eval_count);
    assert!(!out.trim().is_empty(), "llama.cpp produced no tokens");
}

// ── the remote server — locate + spawn the REAL remote_lm.server + generate ────────────────────

#[tokio::test]
#[ignore]
async fn live_remote_spawn_and_generate() {
    use quantamind_lib::inference::remote::server::remote_endpoint::{remote_endpoint, set_remote_port};
    use quantamind_lib::inference::remote::server::remote_locate::locate;
    use quantamind_lib::inference::remote::server::remote_runtime::{build_spawn_args, find_available_port, kill_server, spawn_server};
    use std::path::PathBuf;

    let model = remote_model();
    assert!(PathBuf::from(&model).exists(), "the remote server model dir not found: {model}");
    let exe = locate(None).expect("remote_lm.server not found in PATH/venvs");
    println!("[remote] server exe = {exe:?}");
    let port = find_available_port(8082).expect("no free the remote server port");
    set_remote_port(port);

    let mut child = spawn_server(&exe, &build_spawn_args(&model, port)).expect("spawn remote_lm.server");
    println!("[remote] spawned pid={} on :{port} model={model}", child.id());

    // remote_lm.server has no /health; poll by attempting a tiny generation (weights
    // load can take ~30–60s on first run).
    let ep = remote_endpoint();
    let mut out = String::new();
    let mut ok = false;
    for attempt in 0..60 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        out.clear();
        match quantamind_lib::inference::remote::remote::stream_generate(&ep, &model, "Reply with exactly one short sentence: say hello.", None, None, CancellationToken::new(), |t| out.push_str(t)).await {
            Ok(stats) if !out.trim().is_empty() => {
                println!("[remote] ready after ~{}s  output={:?}  tokens={:?}", attempt * 2, out.trim(), stats.eval_count);
                ok = true;
                break;
            }
            Ok(_) => {} // up but empty — keep polling
            Err(_) => {} // not up yet
        }
    }
    let _ = kill_server(&mut child); // always reap
    assert!(ok, "the remote server server never produced output for {model}");
}

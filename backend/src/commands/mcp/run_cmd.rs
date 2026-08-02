//! Run commands: score a controlled-world task (Track B, pass^k end-state) and
//! grade a model against the user's own server (Track A, schema + attribution).
//! Both take the model + backend from the caller (the global header selection);
//! the endpoint is resolved server-side via `endpoint::resolve`.

use crate::commands::emit::log_emit;
use crate::commands::eval::batch_cmd::BatchRunState;
use crate::commands::eval::batch_payloads::{
    AgenticStepPayload, BatchCompletePayload, BatchProgress, EVENT_AGENTIC_STEP, EVENT_BATCH_COMPLETE,
    EVENT_BATCH_PROGRESS,
};
use crate::commands::mcp::mcp_cmd::{connect_configured, registry_path};
use crate::commands::mcp::task_cmd::{ByoTaskSpec, McpTaskSpec, WorldSpec};
use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::eval::agentic::env_view::EnvView;
use crate::inference::eval::agentic::scoring::report::{AgenticReport, DiagnosticStats, FailureTracker, TopError};
use crate::inference::eval::agentic::spec::Tier;
use crate::inference::eval::agentic::step::{StepKind, TrajectoryStep};
use crate::inference::eval::batch::{
    batch_summaries, AggAgentic, BatchColumn, BatchReport, NativeErrorClass, TaskOutcome,
};
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::score::{score_db_task, score_fs_task, DbTask, McpTask};
use crate::inference::eval::mcp::world::{DbSeed, FsSeed};
use crate::inference::mcp::agent::BackendDriver;
use crate::inference::mcp::bridge::{self, mcp_tools_to_native};
use crate::inference::mcp::gate::{gate_decision, Decision, GatePolicy, PendingCall};
use crate::inference::mcp::oracle_error::Attribution;
use crate::inference::mcp::oracle_schema::{check_call, CallCheck};
use crate::mcp::registry::split_namespaced;
use crate::persistence::mcp::servers::load;
use crate::persistence::readiness::reports;
use crate::persistence::eval_history;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri::Manager;

/// The report/history KEY for a Bring-Your-Own diagnostic run. Not a saved
/// collection — the `mcp:*` id is only how the Simulator/Model-Results/Audit pages
/// address this run's report.
const BYO_COLLECTION: &str = "mcp:byo";

const SYSTEM: &str = "You are a tool-using assistant. Use the provided MCP tools to accomplish the \
task. Call tools with correct arguments and absolute paths where required. When the task is done, \
say so in plain text and stop.";

fn resolve_backend(backend: BackendKind) -> AppResult<String> {
    let url = endpoint::resolve(backend)?.url;
    if url.is_empty() {
        return Err(AppError::Validation("selected backend has no resolved endpoint".into()));
    }
    Ok(url)
}

// ── Track B: controlled-world task ─────────────────────────────────────────

#[derive(Serialize)]
pub struct McpRunResult {
    pub k: u32,
    pub passes: u32,
    pub ready: bool,
    pub pass_rate: f64,
    /// Oracle failures per failed run (the "why not ready").
    pub failures: Vec<Vec<String>>,
}

/// Score a controlled-world task k times against a fresh world each run, driven
/// by the real model. Grades the WORLD end-state (pass^k), never the model's words.
#[tauri::command]
pub async fn run_mcp_world_task(
    model: String,
    backend: BackendKind,
    task: McpTaskSpec,
    max_steps: Option<u32>,
) -> Result<McpRunResult, AppError> {
    let endpoint = resolve_backend(backend)?;
    let k = task.k.max(1) as usize;
    let steps = max_steps.unwrap_or(6).max(1) as usize;
    let instruction = task.instruction.clone();

    let score = match task.world {
        WorldSpec::Fs { files } => {
            let seed = FsSeed {
                files: files.into_iter().map(|f| (f.path, f.content)).collect::<BTreeMap<_, _>>(),
            };
            let oracle = FsOracle {
                assert_present: task.oracle.assert_present,
                assert_absent: task.oracle.assert_absent,
                assert_content: task.oracle.assert_content,
            };
            let mtask = McpTask { instruction: instruction.clone(), seed, oracle };
            let (ep, model) = (endpoint.clone(), model.clone());
            score_fs_task(
                &mtask,
                |root, tools| BackendDriver {
                    backend,
                    endpoint: ep.clone(),
                    model: model.clone(),
                    system: SYSTEM.to_string(),
                    instruction: format!(
                        "{instruction}\n\nWork ONLY inside the directory {}. Use absolute paths under it.",
                        root.display()
                    ),
                    tools_json: mcp_tools_to_native(tools),
                },
                k,
                steps,
            )
            .await?
        }
        WorldSpec::Db { setup_sql } => {
            let oracle =
                DbOracle { assert_eq: task.oracle.assert_eq, assert_contains: task.oracle.assert_contains };
            let dtask = DbTask { instruction: instruction.clone(), seed: DbSeed::new(&setup_sql), oracle };
            let (ep, model) = (endpoint.clone(), model.clone());
            score_db_task(
                &dtask,
                |_db, tools| BackendDriver {
                    backend,
                    endpoint: ep.clone(),
                    model: model.clone(),
                    system: SYSTEM.to_string(),
                    instruction: format!(
                        "{instruction}\n\nUse write_query for INSERT/UPDATE/DELETE and read_query for SELECT."
                    ),
                    tools_json: mcp_tools_to_native(tools),
                },
                k,
                steps,
            )
            .await?
        }
    };

    Ok(McpRunResult {
        k: score.k as u32,
        passes: score.passes as u32,
        ready: score.is_ready(),
        pass_rate: score.pass_rate(),
        failures: score.failures,
    })
}

// ── Track A: Bring-Your-Own (schema + attribution, no answer key) ──────────

#[derive(Serialize)]
pub struct ByoCall {
    pub tool: String,
    pub schema_valid: bool,
    pub attribution: Attribution,
    /// On success, the tool's actual result text (what the server returned); on a fault,
    /// the reason. This is what the Evaluator shows as the sandbox response, so a plain
    /// "ok" is never enough — the real output goes here (truncated).
    pub detail: String,
    /// A short preview of the arguments the model passed, so the trace shows the CALL, not
    /// just the tool name.
    pub args: String,
}

/// Join a tool result's text blocks into a trace-friendly (truncated) string.
fn result_text(res: &crate::mcp::wire::CallToolResult) -> String {
    let joined: String = res
        .content
        .iter()
        .filter_map(|b| match b {
            crate::mcp::wire::ContentBlock::Text { text } => Some(text.trim()),
            _ => None,
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if joined.is_empty() {
        return "ok (no text content)".to_string();
    }
    let truncated: String = joined.chars().take(600).collect();
    if truncated.len() < joined.len() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[derive(Serialize, Default)]
pub struct ByoRunResult {
    pub total_calls: usize,
    pub schema_valid: usize,
    pub schema_valid_rate: f64,
    pub model_faults: usize,
    pub config_faults: usize,
    pub server_faults: usize,
    pub successes: usize,
    pub calls: Vec<ByoCall>,
    pub assistant_text: String,
}

/// The engine behind both BYO commands: run the model once against the user's OWN
/// server and grade each call for schema-validity + whose-fault attribution — no
/// world, no answer key. Kept separate from the `#[tauri::command]` wrappers so the
/// batch adapter can drive it and re-emit the calls as a trace.
async fn run_byo_inner(
    app: &tauri::AppHandle,
    model: &str,
    backend: BackendKind,
    server_id: &str,
    instruction: &str,
    allow_execute: bool,
) -> AppResult<ByoRunResult> {
    let endpoint = resolve_backend(backend)?;
    let cfg = load(&registry_path(app)?)?
        .get(server_id)
        .ok_or_else(|| AppError::NotFound(format!("mcp server '{server_id}'")))?
        .clone();
    let client = connect_configured(&cfg).await?;
    let tools = client.list_tools().await?.tools;

    let native = mcp_tools_to_native(&tools);
    // Nudge the model to USE the tools instead of asking for clarification. A filesystem server
    // hides its sandbox root behind `list_allowed_directories`, so tell the model to discover it
    // first when that tool exists (mirrors the controlled-world prompt).
    let has_discover = tools.iter().any(|t| t.name.contains("list_allowed_directories"));
    let prompt = if has_discover {
        format!(
            "You have filesystem tools scoped to a sandbox directory. FIRST call \
             `list_allowed_directories` to find your working directory, then use the tools to \
             accomplish the task, using absolute paths under it. Act with the tools — do NOT ask \
             for clarification.\n\nTask: {instruction}"
        )
    } else {
        format!(
            "You have MCP tools available. Use them to accomplish the task; act with the tools \
             rather than asking for clarification.\n\nTask: {instruction}"
        )
    };
    let result = bridge::chat(backend, &endpoint, model, SYSTEM, &prompt, &native, None).await?;
    let calls = bridge::select_calls(&result.tool_calls, &result.content);

    let mut out = ByoRunResult { assistant_text: result.content.clone(), ..Default::default() };
    out.total_calls = calls.len();
    for call in &calls {
        let check = check_call(&tools, call);
        let (attribution, detail) = match &check {
            CallCheck::UnknownTool => (Attribution::Model, "hallucinated tool".to_string()),
            CallCheck::Invalid(v) => (Attribution::Model, v.join("; ")),
            CallCheck::Valid => {
                let bare = split_namespaced(&call.name).map(|(_, t)| t).unwrap_or(call.name.as_str());
                // The user's OWN server is real, not a disposable sandbox — so it's fail-closed:
                // a well-formed call still needs an explicit approval before it executes. The
                // per-run "Allow tool execution" opt-in IS that approval; without it the gate
                // denies and the call is recorded schema-valid-but-not-run. `trusted_server` is
                // false (a configured server carries no verified trust yet), so a poisoned
                // `readOnlyHint` can't self-approve a write.
                let read_only = tools
                    .iter()
                    .find(|t| t.name == bare)
                    .and_then(|t| t.annotations.as_ref())
                    .and_then(|a| a.read_only_hint)
                    .unwrap_or(false);
                let pending = PendingCall {
                    server_id: server_id.to_string(),
                    tool: bare.to_string(),
                    read_only,
                    trusted_server: false,
                    args: call.args.clone(),
                };
                match byo_gate_decision(allow_execute, &pending) {
                    Decision::Deny => (
                        Attribution::Blocked,
                        "blocked — real tool calls are denied by default; enable \u{201C}Allow tool execution\u{201D} to run them".to_string(),
                    ),
                    Decision::Approve => match client.call_tool(bare, call.args.clone()).await {
                        Ok(res) if res.is_error() => (Attribution::Server, format!("tool reported an error: {}", result_text(&res))),
                        Ok(res) => (Attribution::Success, result_text(&res)),
                        Err(e) => (Attribution::Config, e.friendly()),
                    },
                }
            }
        };
        if check.is_valid() {
            out.schema_valid += 1;
        }
        match attribution {
            Attribution::Model => out.model_faults += 1,
            Attribution::Config => out.config_faults += 1,
            Attribution::Server => out.server_faults += 1,
            Attribution::Success => out.successes += 1,
            // Schema-valid but not executed — neither a success nor a fault of any party.
            Attribution::Blocked => {}
        }
        let args = call.args.to_string().chars().take(200).collect::<String>();
        out.calls.push(ByoCall { tool: call.name.clone(), schema_valid: check.is_valid(), attribution, detail, args });
    }
    out.schema_valid_rate = if out.total_calls == 0 { 0.0 } else { out.schema_valid as f64 / out.total_calls as f64 };
    client.kill();
    Ok(out)
}

/// Run the model once against the user's OWN server, then grade each call for
/// schema-validity + whose-fault attribution — no world, no answer key.
#[tauri::command]
pub async fn run_mcp_byo(
    app: tauri::AppHandle,
    model: String,
    backend: BackendKind,
    server_id: String,
    instruction: String,
    max_steps: Option<u32>,
    allow_execute: Option<bool>,
) -> Result<ByoRunResult, AppError> {
    let _ = max_steps; // single-turn grading for now
    Ok(run_byo_inner(&app, &model, backend, &server_id, &instruction, allow_execute.unwrap_or(false)).await?)
}

/// The StepKind that honestly describes a graded call, so the Evaluator's trace
/// colours it like any other turn (a Model fault reads as a schema/hallucination
/// turn, a Server/Config fault as a tool error, a Success as a clean tool call).
fn step_kind_for(c: &ByoCall) -> StepKind {
    match c.attribution {
        // A blocked call was a valid tool call that simply didn't run — show it as a
        // tool call (its detail explains the gate denial), not an error.
        Attribution::Success | Attribution::Blocked => StepKind::ToolCall,
        Attribution::Model if c.detail.contains("hallucinated") => StepKind::UnknownTool,
        Attribution::Model => StepKind::SchemaError,
        Attribution::Server | Attribution::Config => StepKind::ToolError,
    }
}

/// The approval decision for one BYO tool call. The user's own server is real (not a
/// disposable sandbox) and carries no verified trust, so it is **deny-by-default**: the
/// per-run `allow_execute` opt-in is the ONLY approval source. Pure, so the security
/// contract — no execution without an explicit opt-in — is unit-tested without a live server.
fn byo_gate_decision(allow_execute: bool, pending: &PendingCall) -> Decision {
    gate_decision(GatePolicy::DenyByDefault, pending, allow_execute.then_some(Decision::Approve))
}

fn add_diag(a: DiagnosticStats, b: &DiagnosticStats) -> DiagnosticStats {
    DiagnosticStats {
        total_calls: a.total_calls + b.total_calls,
        schema_valid: a.schema_valid + b.schema_valid,
        successes: a.successes + b.successes,
        model_faults: a.model_faults + b.model_faults,
        config_faults: a.config_faults + b.config_faults,
        server_faults: a.server_faults + b.server_faults,
    }
}

fn diag_of(out: &ByoRunResult) -> DiagnosticStats {
    DiagnosticStats {
        total_calls: out.total_calls as u32,
        schema_valid: out.schema_valid as u32,
        successes: out.successes as u32,
        model_faults: out.model_faults as u32,
        config_faults: out.config_faults as u32,
        server_faults: out.server_faults as u32,
    }
}

/// Bring-Your-Own, wired into the eval eco-system: run each diagnostic against the
/// user's real server and emit the SAME batch events (`batch-progress` /
/// `agentic-step` / `batch-complete`) + persist a `BatchReport` keyed `mcp:byo`, so the
/// Simulator, Evaluator (live trace) and Model Results light up exactly like a Built-In
/// run. There is NO answer key, so the report carries `DiagnosticStats` (schema-valid
/// rate + attribution), never a pass^k. Registers with `BatchRunState` so the SAME Stop
/// button cancels it mid-run.
#[tauri::command]
pub async fn run_mcp_byo_batch(
    app: tauri::AppHandle,
    state: tauri::State<'_, BatchRunState>,
    model: String,
    backend: BackendKind,
    tasks: Vec<ByoTaskSpec>,
    k: Option<u32>,
    allow_execute: Option<bool>,
) -> Result<(), AppError> {
    let allow_execute = allow_execute.unwrap_or(false);
    let cancel = state.begin();
    let total = tasks.len();
    let runs = k.unwrap_or(1).max(1);
    let mut agg = DiagnosticStats::default();

    'tasks: for (i, task) in tasks.iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        // A task begins — sizes the scoreboard bar + opens the (model, task) cell.
        log_emit(&app, EVENT_BATCH_PROGRESS, BatchProgress::Started {
            collection_id: BYO_COLLECTION.to_string(),
            model: model.clone(), task_id: task.name.clone(), index: i, total, category: "mcp_byo".into(), is_native: false,
        });

        let emit_step = |step: TrajectoryStep| {
            // Same step-END host RSS sample the batch sink takes (BYO bypasses TauriBatchSink).
            let mut step = step;
            if step.resident_bytes.is_none() {
                step.resident_bytes = crate::commands::system::process_memory::backend_rss(backend);
            }
            log_emit(&app, EVENT_AGENTIC_STEP, AgenticStepPayload {
                collection_id: BYO_COLLECTION.to_string(),
                model: model.clone(), task_id: task.name.clone(), is_native: false, step,
            });
        };

        // Repeat the diagnostic over the K iterations from Run Params — a reliability-of-
        // well-formedness sample (does the model consistently make valid calls?). No answer
        // key, so it aggregates schema-valid/attribution across runs, never a pass^k.
        let mut task_diag = DiagnosticStats::default();
        for run in 0..runs as usize {
            // Race the (slow) model + tool run against the Stop button. Dropping the future on
            // cancel also drops the MCP client → its Drop kills the server (no orphan).
            let out = tokio::select! {
                r = run_byo_inner(&app, &model, backend, &task.server_id, &task.instruction, allow_execute) => r?,
                _ = cancel.cancelled() => break 'tasks,
            };

            // Re-emit each graded call as a trajectory step (the Evaluator's live trace).
            if out.calls.is_empty() {
                emit_step(byo_step(run, 0, StepKind::ReportedInProse, &out.assistant_text, None, (run == 0).then_some(task.instruction.as_str())));
            } else {
                for (j, c) in out.calls.iter().enumerate() {
                    // raw_output = the CALL (tool + args); injection = the server's actual response.
                    let raw = if c.args.is_empty() || c.args == "{}" { c.tool.clone() } else { format!("{}({})", c.tool, c.args) };
                    let initial = (run == 0 && j == 0).then_some(task.instruction.as_str());
                    emit_step(byo_step(run, j, step_kind_for(c), &raw, Some(&c.detail), initial));
                }
            }
            task_diag = add_diag(task_diag, &diag_of(&out));
        }

        // The task's terminal outcome — a diagnostic report (schema-valid, no pass^k).
        agg = add_diag(agg, &task_diag);
        log_emit(&app, EVENT_BATCH_PROGRESS, BatchProgress::Done {
            collection_id: BYO_COLLECTION.to_string(),
            model: model.clone(), task_id: task.name.clone(), outcome: TaskOutcome::Agentic { report: byo_report(&task_diag) }, is_native: false,
        });
    }

    // The final batch report (one column, the aggregate diagnostic) → Model Results + persistence.
    let full = BatchReport {
        collection_id: BYO_COLLECTION.to_string(),
        unreadable_columns: 0,
        columns: vec![byo_column(&model, backend, &agg, agg.successes, agg.total_calls)],
        num_ctx: None,
        collection_hash: None, // never publishable — no answer key
        think_preset: None,
        params: None,
    };
    log_emit(&app, EVENT_BATCH_COMPLETE, BatchCompletePayload { report: full.clone(), r#final: true });

    // Persist to history + the batch-report store (best-effort; a disk hiccup must not fail the run).
    if let Ok(dir) = app_subdir(&app, "history") {
        let entries = batch_summaries(&full, &crate::time_iso::now_utc());
        if !entries.is_empty() {
            let _ = eval_history::append(&dir, BYO_COLLECTION, &entries);
        }
    }
    if let Ok(dir) = app_subdir(&app, "batch_reports") {
        let _ = reports::save(&dir, &full);
    }
    Ok(())
}

fn app_subdir(app: &tauri::AppHandle, name: &str) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join(name))
}

/// A default, app-managed SQLite path for the sqlite quick-add, so the user doesn't have to
/// type an absolute path (the #1 sqlite-connect failure). `mcp-server-sqlite-npx` creates the
/// file on first run; we just ensure the parent dir exists.
#[tauri::command]
pub fn mcp_scratch_db_path(app: tauri::AppHandle) -> Result<String, AppError> {
    let dir = app_subdir(&app, "mcp")?;
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(dir.join("scratch.db").to_string_lossy().into_owned())
}

/// One diagnostic trajectory step (no env replay). `run` groups it under "RUN n" in the
/// Evaluator when the diagnostic is repeated over the K iterations from Run Params.
fn byo_step(run: usize, i: usize, kind: StepKind, raw: &str, injection: Option<&str>, initial: Option<&str>) -> TrajectoryStep {
    TrajectoryStep {
        run_index: run as u32,
        step_index: i as u32,
        raw_output: raw.to_string(),
        injection: injection.map(str::to_string),
        kind,
        env: EnvView::None,
        cache_n: None,
        prefill_tokens: None,
        prefill_ms: None,
        eval_ms: None,
        load_ms: None,
        total_ms: None,
        output_tokens: None,
        resident_bytes: None, // sampled at the emit site, not in this pure constructor
        reasoning_tokens: None,
        thinking_split_measured: false,
        context_used: None,
        context_window: None,
        initial_prompt: initial.map(str::to_string),
    }
}

/// The per-task diagnostic report (schema-valid carried distinctly; `passes`/`total_runs`
/// are set but never rendered for a diagnostic — the UI branches on `diagnostic`).
fn byo_report(diag: &DiagnosticStats) -> AgenticReport {
    AgenticReport {
        // BYO/MCP diagnostic: no native tool pass ran, so the channel is unmeasured (not zero).
        native_structured_calls: None,
        native_salvaged_calls: None,
        passes: diag.schema_valid,
        total_runs: diag.total_calls,
        failures: FailureTracker::default(),
        avg_output_tokens_success: None,
        avg_steps: None,
        top_error: TopError::None,
        schema_resilience: None,
        tier: Tier::default(),
        requested_runs: None,
        dialect: Default::default(),
        safety_attribution: Default::default(),
        safety: None,
        output_tokens_total: 0,
        diagnostic: Some(diag.clone()),
        wall_ms: None, // diagnostic adapter — not a timed Pass^k batch
    }
}

#[cfg(test)]
mod byo_report_tests {
    use super::*;

    /// The honesty invariant: a BYO run carries a DIAGNOSTIC, never a pass^k verdict.
    #[test]
    fn byo_report_and_column_are_diagnostic_never_passk() {
        let diag = DiagnosticStats {
            total_calls: 6,
            schema_valid: 5,
            successes: 4,
            model_faults: 1,
            config_faults: 0,
            server_faults: 1,
        };
        let report = byo_report(&diag);
        assert_eq!(report.diagnostic.as_ref().unwrap().schema_valid, 5);
        // 5 valid of 6 calls ≠ a clean pass — and there's no answer key anyway.
        assert!(!report.is_strict_pass(), "a diagnostic must never read as a strict pass");

        let col = byo_column("m", BackendKind::LlamaCpp, &diag, 4, 6);
        let ag = col.agentic.expect("BYO column has an agentic aggregate");
        assert!(ag.diagnostic.is_some(), "the aggregate carries the diagnostic");
        assert_eq!(ag.pass_k(), None, "tasks_total=0 → Model Results shows no pass^k for BYO");
    }

    fn pending(read_only: bool) -> PendingCall {
        PendingCall {
            server_id: "s".into(),
            tool: "write_file".into(),
            read_only,
            trusted_server: false,
            args: serde_json::json!({ "path": "x.txt" }),
        }
    }

    /// The security contract behind issue #192: a BYO tool call against the user's own
    /// (real) server is DENIED unless the user explicitly opted into execution — even a
    /// tool that claims to be read-only, because the server carries no verified trust
    /// (a poisoned `readOnlyHint` must not self-approve).
    #[test]
    fn byo_calls_are_denied_without_the_execute_opt_in() {
        // Default (no opt-in) → deny, whatever the tool claims.
        assert_eq!(byo_gate_decision(false, &pending(false)), Decision::Deny, "write denied by default");
        assert_eq!(byo_gate_decision(false, &pending(true)), Decision::Deny, "read-only claim can't self-approve on an untrusted server");
        // Explicit opt-in → approve (the opt-in IS the required approval).
        assert_eq!(byo_gate_decision(true, &pending(false)), Decision::Approve, "opt-in approves execution");
    }
}

/// The per-model column: `tasks_total: 0` so `pass_k()` is `None` (Model Results shows
/// no pass^k), and the diagnostic aggregate drives the "schema-valid X/Y" cell.
fn byo_column(model: &str, backend: BackendKind, diag: &DiagnosticStats, successes: u32, total: u32) -> BatchColumn {
    BatchColumn {
        model: model.to_string(),
        backend,
        toolcall: None,
        agentic: Some(AggAgentic {
            // BYO/MCP diagnostic: no native tool pass ran → unmeasured, not zero.
            native_structured_calls: None,
            native_salvaged_calls: None,
            tasks_passed: 0,
            tasks_total: 0,
            passes: successes,
            total_runs: total,
            avg_steps: None,
            avg_output_tokens_success: None,
            schema_resilience: None,
            top_error: TopError::None,
            failures: FailureTracker::default(),
            by_tier: vec![],
            tasks_errored: 0,
            native_error_class: NativeErrorClass::None,
            boundary: None,
            tokens_per_completed: None,
            diagnostic: Some(diag.clone()),
        }),
        agentic_native_fc: None,
        error: None,
        is_thinking: false,
        ..Default::default()
    }
}

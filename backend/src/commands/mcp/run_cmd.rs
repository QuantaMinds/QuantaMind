//! Run commands: score a controlled-world task (Track B, pass^k end-state) and
//! grade a model against the user's own server (Track A, schema + attribution).
//! Both take the model + backend from the caller (the global header selection);
//! the endpoint is resolved server-side via `endpoint::resolve`.

use crate::commands::mcp::mcp_cmd::{connect_configured, registry_path};
use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::score::{score_db_task, score_fs_task, DbTask, McpTask};
use crate::inference::eval::mcp::world::{DbSeed, FsSeed};
use crate::inference::mcp::agent::BackendDriver;
use crate::inference::mcp::bridge::{self, mcp_tools_to_native};
use crate::inference::mcp::oracle_error::Attribution;
use crate::inference::mcp::oracle_schema::{check_call, CallCheck};
use crate::mcp::registry::split_namespaced;
use crate::persistence::mcp::servers::load;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// Mirror of the frontend `McpTaskDef` (the one task-file format).
#[derive(Deserialize)]
pub struct McpTaskSpec {
    #[allow(dead_code)]
    pub name: String,
    pub instruction: String,
    pub world: WorldSpec,
    #[serde(default)]
    pub oracle: OracleSpec,
    #[serde(default = "default_k")]
    pub k: u32,
}
fn default_k() -> u32 {
    5
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorldSpec {
    Fs {
        #[serde(default)]
        files: Vec<FileSpec>,
    },
    Db {
        #[serde(default, rename = "setupSql")]
        setup_sql: String,
    },
}

#[derive(Deserialize)]
pub struct FileSpec {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, Default)]
pub struct OracleSpec {
    #[serde(default)]
    pub assert_present: Vec<String>,
    #[serde(default)]
    pub assert_absent: Vec<String>,
    #[serde(default)]
    pub assert_content: Vec<(String, String)>,
    #[serde(default)]
    pub assert_eq: Vec<(String, String)>,
    #[serde(default)]
    pub assert_contains: Vec<(String, String)>,
}

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
    pub detail: String,
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
) -> Result<ByoRunResult, AppError> {
    let _ = max_steps; // single-turn grading for now
    let endpoint = resolve_backend(backend)?;
    let cfg = load(&registry_path(&app)?)?
        .get(&server_id)
        .ok_or_else(|| AppError::NotFound(format!("mcp server '{server_id}'")))?
        .clone();
    let client = connect_configured(&cfg).await?;
    let tools = client.list_tools().await?.tools;

    let native = mcp_tools_to_native(&tools);
    let result = bridge::chat(backend, &endpoint, &model, SYSTEM, &instruction, &native, None).await?;
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
                match client.call_tool(bare, call.args.clone()).await {
                    Ok(res) if res.is_error() => (Attribution::Server, "tool reported isError".to_string()),
                    Ok(_) => (Attribution::Success, "ok".to_string()),
                    Err(e) => (Attribution::Config, e.friendly()),
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
        }
        out.calls.push(ByoCall { tool: call.name.clone(), schema_valid: check.is_valid(), attribution, detail });
    }
    out.schema_valid_rate = if out.total_calls == 0 { 0.0 } else { out.schema_valid as f64 / out.total_calls as f64 };
    client.kill();
    Ok(out)
}

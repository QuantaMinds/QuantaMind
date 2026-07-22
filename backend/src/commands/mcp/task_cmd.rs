//! Stage 2: convert an MCP task (`McpTaskSpec`, the builder's world+oracle form)
//! into an eval `ToolTask` so it runs through the EXISTING agentic runner
//! (Stage 1) and produces a standard `BatchReport`. The tool schemas are the
//! fixed reference-server sets (fs from the captured fixture, sqlite hard-coded),
//! and the prompt tells the model to DISCOVER its per-run sandbox first (the root
//! is only known at run time).


use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::{AgenticSpec, EnvKind};
use crate::inference::eval::mcp::oracle_db::DbOracle;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::{DbSeed, FsSeed, McpSpec};
use crate::inference::eval::toolcall::tasks::{ToolSchema, ToolTask};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

// The MCP task-spec types (the builder's world+oracle wire format) live HERE —
// the spec→ToolTask stage — so the headless CLI parses task files without the
// GUI-only run commands; `run_cmd` (gui) imports them back from this module.
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

/// One BYO task: which connected server + what the model should do. Diagnostic only
/// (no answer key). `camelCase` to match the frontend `McpByoTaskDef`.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ByoTaskSpec {
    pub name: String,
    pub instruction: String,
    pub server_id: String,
}


/// The 14 real filesystem-server tool schemas, parsed from the captured fixture
/// (the same bytes the wire types were modeled from).
fn fs_tool_schemas() -> Vec<ToolSchema> {
    const FIXTURE: &str = include_str!("../../../../fixtures/mcp/tools_list.json");
    let wrapper: serde_json::Value = serde_json::from_str(FIXTURE).unwrap_or_default();
    let tools: Vec<crate::mcp::wire::Tool> =
        serde_json::from_value(wrapper["response"]["result"]["tools"].clone()).unwrap_or_default();
    tools.into_iter().map(tool_to_schema).collect()
}

/// Convert an MCP `Tool` → eval `ToolSchema`, normalizing `parameters` to a valid
/// object schema (`type:"object"` + a `properties` map) so `validate_tool` passes.
fn tool_to_schema(t: crate::mcp::wire::Tool) -> ToolSchema {
    let mut params = t.input_schema;
    if !params.is_object() {
        params = json!({ "type": "object", "properties": {} });
    }
    if let Some(o) = params.as_object_mut() {
        o.entry("type").or_insert_with(|| json!("object"));
        o.entry("properties").or_insert_with(|| json!({}));
    }
    ToolSchema { name: t.name, description: t.description.unwrap_or_default(), parameters: params }
}

/// The 5 sqlite-server (`sqlite-manager`) tools.
fn db_tool_schemas() -> Vec<ToolSchema> {
    let q = || json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] });
    vec![
        ToolSchema { name: "read_query".into(), description: "Run a SELECT query".into(), parameters: q() },
        ToolSchema { name: "write_query".into(), description: "Run an INSERT/UPDATE/DELETE query".into(), parameters: q() },
        ToolSchema { name: "create_table".into(), description: "Run a CREATE TABLE statement".into(), parameters: q() },
        ToolSchema {
            name: "list_tables".into(),
            description: "List the tables in the database".into(),
            parameters: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "describe_table".into(),
            description: "Describe a table's columns".into(),
            parameters: json!({ "type": "object", "properties": { "table_name": { "type": "string" } }, "required": ["table_name"] }),
        },
    ]
}

fn agentic_spec(mcp: McpSpec, recognized: Vec<String>, k: u32, max_steps: u32) -> AgenticSpec {
    AgenticSpec {
        mocks: vec![],
        mcp: Some(mcp),
        end_state: EndStateRule::RequireWorldOracle,
        environment: EnvKind::Mcp,
        tier: Default::default(),
        axes: None,
        k: Some(k.max(1)),
        max_steps: Some(max_steps.max(1)),
        faults: vec![],
        max_recovery: None,
        must_not_call: vec![],
        world_state: None,
        name_faults: vec![],
        generated: false,
        entity_tools: vec![],
        recognized_tools: recognized,
        safety: None,
        payload_noise: false,
        field_projections: Default::default(),
    }
}

/// Convert one MCP task to an eval `ToolTask`.
pub fn to_tooltask(spec: &McpTaskSpec, index: usize) -> ToolTask {
    let (tools, mcp, prompt) = match &spec.world {
        WorldSpec::Fs { files } => {
            let seed = FsSeed {
                files: files.iter().map(|f| (f.path.clone(), f.content.clone())).collect::<BTreeMap<_, _>>(),
            };
            let oracle = fs_oracle(&spec.oracle);
            let prompt = format!(
                "You have filesystem tools scoped to a private sandbox directory. FIRST call \
                 `list_allowed_directories` to find your working directory, then: {}. Use ABSOLUTE \
                 paths under that directory. When done, say so.",
                spec.instruction
            );
            (fs_tool_schemas(), McpSpec::Fs { seed, oracle }, prompt)
        }
        WorldSpec::Db { setup_sql } => {
            let oracle =
                DbOracle { assert_eq: spec.oracle.assert_eq.clone(), assert_contains: spec.oracle.assert_contains.clone() };
            let prompt = format!(
                "You have SQL tools over a database. {}. Use `write_query` for INSERT/UPDATE/DELETE \
                 and `read_query` for SELECT. When done, say so.",
                spec.instruction
            );
            (db_tool_schemas(), McpSpec::Db { seed: DbSeed::new(setup_sql), oracle }, prompt)
        }
    };
    let recognized: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
    let id = format!("mcp-{}-{index}", sanitize(&spec.name));
    ToolTask {
        id,
        category: "agent_loop".into(),
        prompt,
        tools,
        expected: Default::default(),
        agentic: Some(agentic_spec(mcp, recognized, spec.k, 8)),
    }
}

fn fs_oracle(o: &OracleSpec) -> FsOracle {
    FsOracle {
        assert_present: o.assert_present.clone(),
        assert_absent: o.assert_absent.clone(),
        assert_content: o.assert_content.clone(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>().to_lowercase()
}

/// Convert built MCP tasks to eval `ToolTask`s for the unified Run-Batch path.
/// Validates them (the same trust boundary as a custom collection).
#[cfg_attr(feature = "gui", tauri::command)]
pub fn build_mcp_tasks(tasks: Vec<McpTaskSpec>) -> Result<Vec<ToolTask>, crate::errors::AppError> {
    let out: Vec<ToolTask> = tasks.iter().enumerate().map(|(i, t)| to_tooltask(t, i)).collect();
    crate::inference::eval::toolcall::tasks::validate_tasks(&out)?;
    Ok(out)
}

/// Deep-validate pasted/authored MCP world tasks BEFORE they enter the builder —
/// so a broken oracle (vacuous, contradictory, do-nothing-passes) is caught with a
/// readable per-task finding at import time, not as a raw error at run time. Runs
/// the SAME pipeline the collection-import gate uses: structural build → deep
/// validation → world static + live checks (do-nothing against the real world).
/// Returns the `CollectionValidation` the UI already renders. A structural build
/// failure comes back as `ok:false` + `structural_error` (never an IPC Err), so the
/// paste door shows findings instead of a red toast.
#[cfg_attr(feature = "gui", tauri::command)]
pub async fn validate_mcp_tasks(
    tasks: Vec<McpTaskSpec>,
) -> Result<crate::inference::eval::agentic::v2::oracle::CollectionValidation, crate::errors::AppError> {
    use crate::inference::eval::agentic::v2::oracle::{validate_collection_deep, CollectionValidation};
    let built: Vec<ToolTask> = tasks.iter().enumerate().map(|(i, t)| to_tooltask(t, i)).collect();
    // Structural trust boundary — a rejection is a finding to show, not an error.
    if let Err(e) = crate::inference::eval::toolcall::tasks::validate_tasks(&built) {
        return Ok(CollectionValidation { ok: false, structural_error: Some(e.to_string()), tasks: vec![] });
    }
    let mut v = validate_collection_deep(&built).await;
    crate::inference::eval::mcp::validate::merge_world_checks(&mut v, &built, true).await;
    Ok(v)
}

/// Row-only `ToolTask`s for Bring-Your-Own tasks — they give the Simulator a row to
/// render + key the outcome by (`id` == task name), matching the events the BYO adapter
/// emits. They are NOT run through the agentic runner (the diagnostic engine drives the
/// real server directly), so they carry no world/oracle — just a placeholder tool so the
/// `ToolTask` shape is valid.
pub fn to_byo_tooltask(spec: &ByoTaskSpec) -> ToolTask {
    ToolTask {
        id: spec.name.clone(),
        category: "agent_loop".into(),
        prompt: spec.instruction.clone(),
        tools: vec![ToolSchema {
            name: "mcp_tools".into(),
            description: format!("Live tools from your '{}' server (diagnostic — no answer key).", spec.server_id),
            parameters: json!({ "type": "object", "properties": {} }),
        }],
        expected: Default::default(),
        agentic: None,
    }
}

#[cfg_attr(feature = "gui", tauri::command)]
pub fn build_mcp_byo_tasks(tasks: Vec<ByoTaskSpec>) -> Result<Vec<ToolTask>, crate::errors::AppError> {
    Ok(tasks.iter().map(to_byo_tooltask).collect())
}

#[cfg(test)]
#[path = "task_cmd_tests.rs"]
mod tests;

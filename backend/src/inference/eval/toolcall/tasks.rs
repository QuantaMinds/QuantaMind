use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::AgenticSpec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A tool the model may call: name, a one-line description, and a JSON-Schema
/// `parameters` object (`{ "type": "object", "properties": {…}, "required": […] }`)
/// — the shape developers paste from their real tool definitions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A concrete tool call: a tool name + its argument object.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Call {
    pub name: String,
    pub args: Value,
}

/// What a correct model should do. `no_call` = the prompt needs no tool
/// (abstention). Internally tagged for readable fixtures.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Expected {
    Call(Call),
    Parallel { calls: Vec<Call> },
    #[default]
    NoCall,
}

impl Expected {
    /// The expected calls, or `None` for an abstention task.
    pub fn calls(&self) -> Option<&[Call]> {
        match self {
            Expected::NoCall => None,
            Expected::Call(c) => Some(std::slice::from_ref(c)),
            Expected::Parallel { calls } => Some(calls),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolTask {
    pub id: String,
    pub category: String,
    pub prompt: String,
    pub tools: Vec<ToolSchema>,
    /// The single-turn success criterion. Defaults to `NoCall` so an agentic task
    /// (which scores via its `agentic.end_state`) need not author one.
    #[serde(default)]
    pub expected: Expected,
    /// Present only for `category == "agentic"` tasks: sandbox mocks, the success
    /// criterion, and Pass^k/step-cap. `skip_serializing_if` keeps single-turn
    /// collections byte-for-byte unchanged on save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agentic: Option<AgenticSpec>,
}

const CATEGORIES: [&str; 6] = ["single", "parallel", "select", "abstain", "agentic", "agent_loop"];

/// Categories that run on the multi-turn agentic engine (score via
/// `agentic.end_state`, not `expected`). Phase 9-v2 added `agent_loop` — the v2
/// authored scenarios — alongside the original `agentic`; both route identically.
pub fn is_agentic(category: &str) -> bool {
    category == "agentic" || category == "agent_loop"
}

/// A tool's `parameters` must be a JSON-Schema object: `type:"object"`, a
/// `properties` map, and an optional `required` list. Used only to validate
/// the untyped `Value` — let serde do the structural work, not hand traversal.
#[derive(Deserialize)]
struct StrictParameters {
    #[serde(rename = "type")]
    schema_type: String,
    properties: BTreeMap<String, Value>,
    #[serde(default)]
    required: Option<Vec<String>>,
}

fn bad(id: &str, field: &str, why: &str) -> AppError {
    AppError::InvalidTaskSchema(format!("task '{id}' {field}: {why}"))
}

fn validate_tool(id: &str, tool: &ToolSchema) -> AppResult<()> {
    if tool.name.trim().is_empty() {
        return Err(bad(id, "tools", "a tool has an empty name"));
    }
    let p: StrictParameters = serde_json::from_value(tool.parameters.clone())
        .map_err(|e| bad(id, &format!("tool '{}' parameters", tool.name), &e.to_string()))?;
    if p.schema_type != "object" {
        return Err(bad(id, &format!("tool '{}' parameters", tool.name), "type must be \"object\""));
    }
    for r in p.required.iter().flatten() {
        if !p.properties.contains_key(r) {
            return Err(bad(id, &format!("tool '{}' parameters", tool.name), &format!("required '{r}' is not a declared property")));
        }
    }
    Ok(())
}

/// Validate a task collection from ANY source (built-in, saved, imported,
/// hand-edited) — the single backend-side trust boundary. Rejects empty sets,
/// empty tools, unknown categories, a malformed `parameters` schema, and an
/// `expected` shape that disagrees with the category.
pub fn validate_tasks(tasks: &[ToolTask]) -> AppResult<()> {
    if tasks.is_empty() {
        return Err(AppError::InvalidTaskSchema("a collection needs at least one task".into()));
    }
    for t in tasks {
        if t.id.trim().is_empty() {
            return Err(AppError::InvalidTaskSchema("a task has an empty id".into()));
        }
        if !CATEGORIES.contains(&t.category.as_str()) {
            return Err(bad(&t.id, "category", &format!("unknown '{}'", t.category)));
        }
        if t.tools.is_empty() {
            return Err(bad(&t.id, "tools", "must offer at least one tool"));
        }
        for tool in &t.tools {
            validate_tool(&t.id, tool)?;
        }
        if is_agentic(&t.category) {
            // The agentic path scores via `agentic.end_state`, not `expected`, so
            // the single-turn abstain/expected rule below is skipped entirely.
            let spec = t
                .agentic
                .as_ref()
                .ok_or_else(|| bad(&t.id, "agentic", "an agentic task requires an agentic spec"))?;
            validate_agentic(&t.id, &t.tools, spec)?;
        } else {
            if t.agentic.is_some() {
                return Err(bad(&t.id, "agentic", "only agentic tasks may carry an agentic spec"));
            }
            let abstain = t.category == "abstain";
            if abstain != matches!(t.expected, Expected::NoCall) {
                return Err(bad(&t.id, "expected", "abstain ⇔ no_call (category and expected disagree)"));
            }
            if let Some(calls) = t.expected.calls() {
                for c in calls {
                    if !t.tools.iter().any(|tool| tool.name == c.name) {
                        return Err(bad(&t.id, "expected", &format!("calls unknown tool '{}'", c.name)));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Validate an agentic task's spec: every end-state checkpoint and mock must name
/// a declared tool, a `require_sequence` needs ≥1 checkpoint, and k/max_steps (if
/// set) must be ≥1. The single backend-side trust boundary for agentic tasks.
fn validate_agentic(id: &str, tools: &[ToolSchema], spec: &AgenticSpec) -> AppResult<()> {
    let known = |name: &str| tools.iter().any(|t| t.name == name);
    match &spec.end_state {
        EndStateRule::RequireSequence(cps) | EndStateRule::RequireAll(cps) => {
            if cps.is_empty() {
                return Err(bad(id, "agentic", "end-state needs at least one checkpoint"));
            }
            for cp in cps {
                if !known(&cp.tool) {
                    return Err(bad(id, "agentic", &format!("end-state checkpoint calls unknown tool '{}'", cp.tool)));
                }
            }
        }
        // RequireEndState grades on the final UI state, not tool checkpoints — the target must be
        // a JSON object (the goal sub-state), and there's nothing tool-named to validate here.
        EndStateRule::RequireEndState(target) => {
            if !target.is_object() {
                return Err(bad(id, "agentic", "require_end_state target must be a JSON object"));
            }
        }
        EndStateRule::ExpectAbstainingText => {}
        // MCP: no in-spec checkpoints; success is the real world's oracle. Require the mcp spec.
        EndStateRule::RequireWorldOracle => {
            if spec.mcp.is_none() {
                return Err(bad(id, "agentic", "MCP end-state (require_world_oracle) needs an `mcp` spec"));
            }
        }
    }
    for m in &spec.mocks {
        if !known(&m.call.name) {
            return Err(bad(id, "agentic", &format!("mock references unknown tool '{}'", m.call.name)));
        }
    }
    // v2: every must_not_call entry must name a declared tool (real or decoy) — a
    // typo'd trap silently never fires and the task becomes easier than authored.
    for trap in &spec.must_not_call {
        let name = match trap {
            crate::inference::eval::agentic::v2::r#match::MustNotCall::Name(n) => n,
            crate::inference::eval::agentic::v2::r#match::MustNotCall::Pair { name, .. } => name,
        };
        if !known(name) {
            return Err(bad(id, "agentic", &format!("must_not_call names unknown tool '{name}'")));
        }
    }
    if matches!(spec.k, Some(0)) || matches!(spec.max_steps, Some(0)) {
        return Err(bad(id, "agentic", "k and max_steps must be >= 1"));
    }
    Ok(())
}

/// Tasks for a built-in collection by id (the v2 scenario file stem, e.g.
/// "easy-coding"), or `None` for an unknown id. Phase 9-v2: the bundled tiered
/// scenarios ARE the eval content — the old hand-coded single/multi fixtures
/// (curated/finance/agentic*) were removed.
pub fn builtin_collection(id: &str) -> Option<Vec<ToolTask>> {
    use crate::inference::eval::agentic::v2::collection::load_v2_collection;
    use crate::inference::eval::agentic::v2::scenarios::v2_json;
    v2_json(id).and_then(|json| load_v2_collection(json).ok())
}

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod tests;

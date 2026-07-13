use crate::errors::{AppError, AppResult};
use crate::inference::eval::agentic::sandbox::{EndStateRule, TaskCheckpoint};
use crate::inference::eval::agentic::spec::{AgenticSpec, DifficultyAxes, EnvKind, FaultInjection, NameFault, SafetySpec, Tier};
use crate::inference::eval::agentic::v2::r#match::MustNotCall;
use crate::inference::eval::toolcall::tasks::{ToolSchema, ToolTask};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// One v2 task as authored (see `SCHEMA.md`). Metadata-only fields (`trap`,
/// `rubric`) are intentionally not modeled — they document intent, not behavior.
#[derive(Deserialize)]
pub struct V2Task {
    pub id: String,
    #[serde(default)]
    pub category: String,
    pub max_steps: u32,
    #[serde(default = "default_recovery")]
    pub max_recovery: u8,
    pub prompt: String,
    #[serde(default)]
    pub world_state: Value,
    pub tools: Vec<V2Tool>,
    #[serde(default)]
    pub decoy_tools: Vec<V2Tool>,
    #[serde(default)]
    pub expected_calls: Vec<V2ExpectedCall>,
    /// Slice 3 (stateful web-UI): the target end state. When present, the task is graded on the
    /// final UI state matching this sub-state (`RequireEndState`), NOT on `expected_calls`
    /// (those become the oracle's drive script in tests). Absent → checkpoint grading.
    #[serde(default)]
    pub target_state: Option<Value>,
    #[serde(default)]
    pub must_not_call: Vec<MustNotCall>,
    #[serde(default)]
    pub faults: Vec<V2Fault>,
    /// Category K: present only on a safety/boundary probe (attack or benign control).
    /// Absent on every capability task (back-compat with existing collections).
    #[serde(default)]
    pub safety: Option<SafetySpec>,
    /// When true, the sandbox wraps world_state getter blobs in a messy envelope so the model
    /// must extract the right field from noise. Absent → false (back-compat).
    #[serde(default)]
    pub payload_noise: bool,
}

fn default_recovery() -> u8 {
    2
}

/// A v2 tool/decoy: `params` is a `{name: type-string}` map (not JSON-Schema).
/// `returns_entity` declares whether the tool surfaces world_state entity data (a
/// GETTER) or merely acts (an ACTION that acks). Absent → getter (back-compat: the
/// pre-field behavior echoed for every tool). Action tools are tagged `false` so they
/// can't hand the model the field it was supposed to reason to (answer-leniency).
#[derive(Deserialize)]
pub struct V2Tool {
    pub name: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub returns_entity: Option<bool>,
    /// Field-scoped getter: the subset of the resolved entity blob this getter surfaces
    /// (models a real endpoint that returns part of a resource). Absent → the whole blob.
    #[serde(default)]
    pub returns_fields: Option<Vec<String>>,
}

impl V2Tool {
    /// A tool surfaces entity data unless explicitly tagged `returns_entity: false`.
    fn is_getter(&self) -> bool {
        self.returns_entity != Some(false)
    }
}

/// A type-tagged expected call. Only `call` is supported; `parallel`/`none` are
/// defined in the schema but unused by authored content and are rejected loudly.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum V2ExpectedCall {
    Call {
        name: String,
        #[serde(default)]
        args: Value,
    },
    Parallel {
        #[serde(default)]
        calls: Vec<Value>,
    },
    None,
}

/// A v2 fault: `on_call` is a tool NAME (trips on any args).
#[derive(Deserialize)]
pub struct V2Fault {
    pub on_call: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub status_code: u16,
    #[serde(default)]
    pub clears_after: u8,
}

impl V2Fault {
    fn to_injection(&self) -> AppResult<FaultInjection> {
        match self.kind.as_str() {
            "transient" => Ok(FaultInjection::TransientError { status_code: self.status_code, clears_after: self.clears_after }),
            "persistent" => Ok(FaultInjection::PersistentError { status_code: self.status_code }),
            other => Err(AppError::InvalidTaskSchema(format!("unknown fault type '{other}'"))),
        }
    }
}

fn map_type(ty: &str) -> &str {
    match ty {
        "string" | "number" | "integer" | "boolean" | "object" | "array" => ty,
        _ => "string",
    }
}

/// `{name: type-string}` → a JSON-Schema object. No `required` (lenient): the
/// end-state match is the real grader, so we only type-check provided args.
fn to_tool_schema(t: &V2Tool) -> ToolSchema {
    let props: Map<String, Value> =
        t.params.iter().map(|(k, ty)| (k.clone(), json!({ "type": map_type(ty) }))).collect();
    ToolSchema {
        name: t.name.clone(),
        description: format!("Agent tool '{}'.", t.name),
        parameters: json!({ "type": "object", "properties": Value::Object(props) }),
    }
}

/// Transpile one v2 task into the engine's `ToolTask{category:"agent_loop", agentic}`.
/// `expected_calls` → `RequireAll` (or `ExpectAbstainingText` when empty); authored
/// `decoy_tools` merge into the presented tool list; `faults` become name-keyed;
/// `world_state` drives the responder.
pub fn transpile_task(
    t: V2Task,
    tier: Tier,
    pass_k: u32,
    axes: DifficultyAxes,
    generated: bool,
    environment: EnvKind,
) -> AppResult<ToolTask> {
    let mut tools: Vec<ToolSchema> = t.tools.iter().map(to_tool_schema).collect();
    tools.extend(t.decoy_tools.iter().map(to_tool_schema));

    // The getter set the WorldState responder consults: real tools that surface entity
    // data. Decoys are never getters (they ack), so they're excluded.
    let entity_tools: Vec<String> =
        t.tools.iter().filter(|tool| tool.is_getter()).map(|tool| tool.name.clone()).collect();
    // The whitelist of recognized real tools (getters + actions), taken BEFORE the decoy
    // extend above — so a call to a decoy/unknown tool gets the corrective nudge, not a
    // misleading `{"ok":true}` ack.
    let recognized_tools: Vec<String> = t.tools.iter().map(|tool| tool.name.clone()).collect();

    // Field-scoped getters: a getter with authored `returns_fields` surfaces only that subset
    // of the resolved entity blob (so a model can't read one fact from another endpoint's call).
    let field_projections: BTreeMap<String, Vec<String>> = t
        .tools
        .iter()
        .filter_map(|tool| tool.returns_fields.clone().map(|f| (tool.name.clone(), f)))
        .collect();

    let mut checkpoints = Vec::with_capacity(t.expected_calls.len());
    for ec in t.expected_calls {
        match ec {
            V2ExpectedCall::Call { name, args } => checkpoints.push(TaskCheckpoint { tool: name, args }),
            V2ExpectedCall::Parallel { .. } | V2ExpectedCall::None => {
                return Err(AppError::InvalidTaskSchema(format!(
                    "task '{}' uses an unsupported expected_call type (parallel/none)",
                    t.id
                )));
            }
        }
    }
    // A `target_state` (stateful web-UI) grades on the final state and overrides checkpoint
    // grading — `expected_calls` then serve only as the oracle's drive script (in tests).
    let end_state = match t.target_state {
        Some(target) => EndStateRule::RequireEndState(target),
        None if checkpoints.is_empty() => EndStateRule::ExpectAbstainingText,
        None => EndStateRule::RequireAll(checkpoints),
    };

    let name_faults = t
        .faults
        .iter()
        .map(|f| Ok(NameFault { on_call: f.on_call.clone(), fault: f.to_injection()? }))
        .collect::<AppResult<Vec<_>>>()?;

    let world_state = if t.world_state.is_null() { None } else { Some(t.world_state) };
    let safety = t.safety;
    let payload_noise = t.payload_noise;

    let spec = AgenticSpec {
        mocks: vec![],
        end_state,
        environment,
        tier,
        axes: Some(axes),
        k: Some(pass_k),
        max_steps: Some(t.max_steps),
        faults: vec![],
        max_recovery: Some(t.max_recovery),
        must_not_call: t.must_not_call,
        world_state,
        name_faults,
        generated,
        entity_tools,
        recognized_tools,
        safety,
        payload_noise,
        field_projections,
    };
    // All v2 tasks run on the agentic engine; the end-state (RequireAll vs
    // ExpectAbstainingText) — not the authored label — encodes act-vs-abstain.
    Ok(ToolTask { id: t.id, category: "agent_loop".into(), prompt: t.prompt, tools, expected: Default::default(), agentic: Some(spec) })
}

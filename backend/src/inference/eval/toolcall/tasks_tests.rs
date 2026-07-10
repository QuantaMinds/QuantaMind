use super::*;
use crate::inference::eval::agentic::sandbox::{EndStateRule, MockResponse, TaskCheckpoint};
use crate::inference::eval::agentic::spec::AgenticSpec;
use crate::inference::eval::toolcall::tasks::Call;
use serde_json::json;

fn valid_task() -> ToolTask {
    ToolTask {
        id: "w".into(),
        category: "single".into(),
        prompt: "Weather in Paris?".into(),
        tools: vec![ToolSchema {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }),
        }],
        expected: Expected::Call(Call { name: "get_weather".into(), args: json!({ "city": "Paris" }) }),
        agentic: None,
    }
}

#[test]
fn builtin_collection_routes_v2_scenario_ids() {
    // Phase 9-v2: the bundled tiered scenarios are the eval content; the old
    // hand-coded presets (curated/finance/agentic*) were removed.
    let easy = builtin_collection("easy-coding").expect("easy-coding is a bundled v2 collection");
    assert!(easy.iter().all(|t| t.category == "agent_loop"));
    assert!(builtin_collection("curated").is_none()); // old preset gone
    assert!(builtin_collection("nope").is_none());
}

#[test]
fn rejects_empty_collection() {
    assert!(matches!(validate_tasks(&[]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_empty_tools() {
    let mut t = valid_task();
    t.tools.clear();
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_unknown_category() {
    let mut t = valid_task();
    t.category = "smoke".into();
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_required_naming_undeclared_property() {
    let mut t = valid_task();
    t.tools[0].parameters = json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["country"] });
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_parameters_missing_properties() {
    let mut t = valid_task();
    t.tools[0].parameters = json!({ "type": "object" });
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_category_expected_mismatch() {
    let mut t = valid_task();
    t.category = "abstain".into(); // but expected is a Call
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_call_to_unoffered_tool() {
    let mut t = valid_task();
    t.expected = Expected::Call(Call { name: "send_email".into(), args: json!({}) });
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

// --- Agentic data-model (Phase 1) -----------------------------------------

fn agentic_valid() -> ToolTask {
    let tool = |n: &str| ToolSchema {
        name: n.into(),
        description: "d".into(),
        parameters: json!({ "type": "object", "properties": {} }),
    };
    ToolTask {
        id: "fin".into(),
        category: "agentic".into(),
        prompt: "Check the balance then transfer it.".into(),
        tools: vec![tool("get_balance"), tool("transfer")],
        expected: Default::default(), // unused by the agentic path
        agentic: Some(AgenticSpec {
            mocks: vec![MockResponse {
                call: Call { name: "get_balance".into(), args: json!({ "id": "A" }) },
                response: "{}".into(),
            }],
            end_state: EndStateRule::RequireSequence(vec![TaskCheckpoint {
                tool: "transfer".into(),
                args: json!({ "amount": 1.0 }),
            }]),
            environment: Default::default(),
            tier: Default::default(),
            axes: None,
            k: None,
            max_steps: None,
            faults: vec![],
            max_recovery: None,
            must_not_call: vec![],
            world_state: None,
            name_faults: vec![],
            generated: false,
            entity_tools: vec![],
            recognized_tools: vec![],
            safety: None,
        }),
    }
}

#[test]
fn agentic_task_validates() {
    validate_tasks(&[agentic_valid()]).expect("valid agentic task");
}

#[test]
fn rejects_agentic_without_spec() {
    let mut t = agentic_valid();
    t.agentic = None;
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn agent_loop_category_validates_like_agentic() {
    use crate::inference::eval::toolcall::tasks::is_agentic;
    assert!(is_agentic("agentic") && is_agentic("agent_loop"));
    assert!(!is_agentic("single") && !is_agentic("abstain"));
    let mut t = agentic_valid();
    t.id = "v2".into();
    t.category = "agent_loop".into(); // the Phase 9-v2 category
    validate_tasks(&[t]).expect("agent_loop validates on the agentic path");
}

#[test]
fn rejects_agent_loop_without_spec() {
    let mut t = agentic_valid();
    t.category = "agent_loop".into();
    t.agentic = None;
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_spec_on_non_agentic_task() {
    let mut t = agentic_valid();
    t.category = "single".into();
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_end_state_checkpoint_naming_unknown_tool() {
    let mut t = agentic_valid();
    if let Some(s) = t.agentic.as_mut() {
        s.end_state = EndStateRule::RequireSequence(vec![TaskCheckpoint { tool: "ghost".into(), args: json!({}) }]);
    }
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn rejects_mock_referencing_unknown_tool() {
    let mut t = agentic_valid();
    if let Some(s) = t.agentic.as_mut() {
        s.mocks = vec![MockResponse { call: Call { name: "ghost".into(), args: json!({}) }, response: "{}".into() }];
    }
    assert!(matches!(validate_tasks(&[t]), Err(AppError::InvalidTaskSchema(_))));
}

#[test]
fn abstaining_agentic_task_needs_no_checkpoints() {
    let mut t = agentic_valid();
    if let Some(s) = t.agentic.as_mut() {
        s.end_state = EndStateRule::ExpectAbstainingText;
        s.mocks.clear();
    }
    validate_tasks(&[t]).expect("expect_abstaining_text is valid with no checkpoints");
}

#[test]
fn single_turn_serialization_omits_the_agentic_key() {
    // The skip_serializing_if guard is load-bearing: a single-turn task (agentic
    // None) must serialize with no `"agentic"` key.
    let json = serde_json::to_string(&valid_task()).unwrap();
    assert!(!json.contains("\"agentic\""), "single-turn tasks must not serialize an agentic key");
}

#[test]
fn agentic_task_round_trips_through_serde() {
    let original = agentic_valid();
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"agentic\""));
    let back: ToolTask = serde_json::from_str(&json).unwrap();
    assert_eq!(original, back);
}

#[test]
fn an_easy_agentic_task_saves_without_leaking_tier_or_axes_keys() {
    // Back-compat: an Easy/None agentic spec re-serializes with no new keys — a
    // saved collection stays byte-compatible.
    let json = serde_json::to_string(&agentic_valid()).unwrap();
    assert!(!json.contains("\"tier\""), "default Easy tier must be omitted on save");
    assert!(!json.contains("\"axes\""), "absent axes must be omitted on save");
}

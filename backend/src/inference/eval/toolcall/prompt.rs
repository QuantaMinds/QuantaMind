use crate::inference::eval::toolcall::tasks::{ToolSchema, ToolTask};
use serde_json::json;

/// How the final answer must be delivered — the closing line of the tool system
/// prompt. Agentic ACT-tasks (`RequireAll`/`RequireSequence`) require every result,
/// including the final report, to go THROUGH a tool; without this a model that reports
/// the correct answer in plain text (as `PlainTextOk` invites) fails as a hallucination
/// even though it did the task. Abstain-tasks (`ExpectAbstainingText`) and single-turn
/// tool-selection keep the plain-text option — there, prose is the correct output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalGuidance {
    /// "If no tool is needed, just answer the user in plain text."
    PlainTextOk,
    /// "Deliver every result — including your final answer — by calling a tool."
    MustUseTools,
}

/// Build the system prompt for a single-turn tool-call task. Single-turn selection
/// allows a plain-text answer when no tool applies, so it keeps `PlainTextOk`.
pub fn build_system(task: &ToolTask) -> String {
    build_system_for(&task.tools, TerminalGuidance::PlainTextOk)
}

/// The task's reporter tool — the first tool whose JSON-schema parameters declare a `text`
/// field (the reporter contract: `reply`/`reply_customer` both take `{text}`). `None` for an
/// action-only toolset, whose deliverable IS its actions. Public so the scenarios integrity
/// test can assert exactly one reporter per reporter task and none on action-only tasks —
/// guarding "first" against a future text-bearing action tool that would mis-resolve here.
pub fn reply_tool_name(tools: &[ToolSchema]) -> Option<&str> {
    tools
        .iter()
        .find(|t| t.parameters.get("properties").and_then(|p| p.get("text")).is_some())
        .map(|t| t.name.as_str())
}

/// The tool-schema-injection core, given the tools + how the final answer must be
/// delivered. Shared so the agentic runner (which has a sandbox, not a `ToolTask`)
/// builds the identical prompt, gating the closing line on its task's end_state.
/// The closing answer-delivery mandate, gated on act-vs-abstain and the toolset's reporter
/// tool. Extracted so BOTH the prompt path (`build_system_for`, which wraps it with JSON-format
/// instructions) and the NATIVE path (`NativeToolTurn`, which uses ONLY this mandate — native
/// tool calls need no JSON instructions) share one source of truth. An act-task with a reporter
/// tool is told to use it BY NAME; an action-only act-task is told its actions ARE the answer
/// (so the mandate never points at a `reply` tool that doesn't exist → no phantom call).
pub fn terminal_closing(tools: &[ToolSchema], terminal: TerminalGuidance) -> String {
    match terminal {
        TerminalGuidance::PlainTextOk => "If no tool is needed, just answer the user in plain text.".into(),
        TerminalGuidance::MustUseTools => match reply_tool_name(tools) {
            Some(name) => format!(
                "Deliver every result by calling a tool. To report your final answer to the user, \
                 call the `{name}` tool. Do not answer in plain text."
            ),
            None => "Deliver every result by calling a tool from the list above — your tool actions \
                     are your final answer. Do not answer in plain text, and do not call a tool that \
                     is not listed."
                .into(),
        },
    }
}

pub fn build_system_for(tools: &[ToolSchema], terminal: TerminalGuidance) -> String {
    let closing: String = terminal_closing(tools, terminal);
    let tools_json = serde_json::to_string_pretty(
        &tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "parameters": t.parameters }))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();
    format!(
        "You can call tools. Available tools:\n{tools_json}\n\n\
         When a tool is needed, respond with ONLY a JSON object of the form \
         {{\"name\": \"<tool>\", \"args\": {{...}}}}. To call several tools, respond \
         with a JSON array of such objects. Do not add prose, explanation, or \
         markdown around the JSON. {closing}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::eval::toolcall::tasks::{Call, Expected};
    use serde_json::json;

    fn tool(name: &str, props: serde_json::Value) -> ToolSchema {
        ToolSchema { name: name.into(), description: format!("{name} tool"), parameters: json!({ "type": "object", "properties": props }) }
    }

    /// A multi-tool task (mirrors the old "select-email" fixture shape).
    fn multi_tool_task() -> ToolTask {
        ToolTask {
            id: "select-email".into(),
            category: "select".into(),
            prompt: "Email the team the weather.".into(),
            tools: vec![
                tool("get_weather", json!({ "city": { "type": "string" } })),
                tool("send_email", json!({ "to": { "type": "string" }, "subject": { "type": "string" } })),
                tool("search_web", json!({ "q": { "type": "string" } })),
            ],
            expected: Expected::Call(Call { name: "send_email".into(), args: json!({ "to": "team", "subject": "wx" }) }),
            agentic: None,
        }
    }

    #[test]
    fn prompt_lists_all_tool_names() {
        let p = build_system(&multi_tool_task());
        for name in ["get_weather", "send_email", "search_web"] {
            assert!(p.contains(name), "missing tool {name}");
        }
    }

    #[test]
    fn prompt_includes_param_names_and_types() {
        let p = build_system(&multi_tool_task());
        assert!(p.contains("\"to\""));
        assert!(p.contains("\"subject\""));
        assert!(p.contains("string"));
    }

    #[test]
    fn instruction_requests_json_only_and_mentions_arrays() {
        let p = build_system(&multi_tool_task());
        assert!(p.contains("ONLY a JSON object"));
        assert!(p.contains("JSON array"));
        assert!(p.contains("plain text"));
    }

    #[test]
    fn plain_text_ok_keeps_the_plain_text_escape_hatch() {
        let tools = multi_tool_task().tools;
        let p = build_system_for(&tools, TerminalGuidance::PlainTextOk);
        assert!(p.contains("just answer the user in plain text"));
        assert!(!p.contains("Do not answer in plain text"));
    }

    #[test]
    fn must_use_tools_on_an_action_only_toolset_forbids_plain_text_and_names_no_reply_tool() {
        // multi_tool_task has NO reporter tool (no `text` param) → action-only wording.
        // The follow-up fix: the mandate must NOT mention a `reply` tool that doesn't exist
        // (that's what induced the phantom reply call on branch_target).
        let tools = multi_tool_task().tools;
        let p = build_system_for(&tools, TerminalGuidance::MustUseTools);
        assert!(p.contains("by calling a tool"));
        assert!(p.contains("Do not answer in plain text"));
        assert!(p.contains("your tool actions are your final answer"));
        assert!(!p.contains("`reply` tool"), "action-only mandate must not point at a nonexistent reply tool: {p}");
        assert!(!p.contains("just answer the user in plain text"));
    }

    #[test]
    fn must_use_tools_names_the_actual_reporter_tool_per_family() {
        // Precision fix: name the REAL reporter — `reply` for coding/math, `reply_customer`
        // for support/ecommerce/finance — never a hard-coded `reply`.
        let reply_tools = vec![tool("get_x", json!({ "id": { "type": "string" } })), tool("reply", json!({ "text": { "type": "string" } }))];
        let p = build_system_for(&reply_tools, TerminalGuidance::MustUseTools);
        assert!(p.contains("call the `reply` tool"), "{p}");
        assert!(p.contains("Do not answer in plain text"));
        assert!(!p.contains("just answer the user in plain text"));

        let rc_tools = vec![tool("get_order", json!({ "id": { "type": "string" } })), tool("reply_customer", json!({ "text": { "type": "string" } }))];
        let p2 = build_system_for(&rc_tools, TerminalGuidance::MustUseTools);
        assert!(p2.contains("call the `reply_customer` tool"), "must name reply_customer, not reply: {p2}");
        assert!(!p2.contains("call the `reply` tool"));
    }

    #[test]
    fn reply_tool_name_detects_the_text_bearing_tool_or_none() {
        let action_only = vec![tool("open_pr", json!({ "change": { "type": "string" }, "base": { "type": "string" } }))];
        assert_eq!(reply_tool_name(&action_only), None);
        let with_reply = vec![tool("act", json!({ "id": { "type": "string" } })), tool("reply_customer", json!({ "text": { "type": "string" } }))];
        assert_eq!(reply_tool_name(&with_reply), Some("reply_customer"));
    }
}

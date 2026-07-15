//! Single-turn bridge: offer a model the MCP tools, take its call(s), execute
//! each against a real MCP server, return the (inert) result. Dispatches by
//! backend — Ollama `/api/chat` and llama.cpp OpenAI `/v1` both return the shared
//! `ChatResult`; MLX has no native tool API. ALL model output is untrusted
//! (OWASP-LLM): results are returned as inert text, never executed.

use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::generate::generate_options::GenerateOptions;
use crate::inference::llama::llama_chat;
use crate::inference::ollama::ollama_chat::{self, ChatResult, NativeToolCall};
use crate::mcp::client::McpClient;
use crate::mcp::registry::split_namespaced;
use crate::mcp::wire::{CallToolResult, ContentBlock, Tool};
use serde::Serialize;
use serde_json::{json, Value};

/// Build the native `tools` array (`/api/chat` + OpenAI `/v1`) from MCP tools:
/// MCP `inputSchema` maps 1:1 to `function.parameters`.
pub fn mcp_tools_to_native(tools: &[Tool]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description.clone().unwrap_or_default(),
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect(),
    )
}

/// How reliable a model is at native tool-calling — for a user-facing warning.
/// Local models below ~3B / heavily quantized often *describe* an action or emit
/// code instead of calling the tool, which would under-report their real ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    Reliable,
    Weak,
    Unknown,
}

const RELIABLE_FAMILIES: &[&str] = &[
    "qwen2.5", "qwen3", "llama3.1", "llama-3.1", "llama3.2", "llama-3.2", "llama3.3", "mistral",
    "hermes", "command-r", "firefunction",
];

fn is_small_model(m: &str) -> bool {
    const SMALL: &[&str] = &["0.5b", "1b", "1.5b", "2b", "3b"];
    // Require a `:` or `-` delimiter so "11b"/"70b" don't match "1b".
    SMALL.iter().any(|s| m.contains(&format!(":{s}")) || m.contains(&format!("-{s}")))
}

pub fn assess_tool_capability(model: &str) -> ToolCapability {
    let m = model.to_lowercase();
    if is_small_model(&m) {
        return ToolCapability::Weak;
    }
    if RELIABLE_FAMILIES.iter().any(|r| m.contains(r)) {
        return ToolCapability::Reliable;
    }
    ToolCapability::Unknown
}

/// A warning to show before trusting a run, or `None` for a reliable model.
pub fn capability_warning(model: &str) -> Option<String> {
    match assess_tool_capability(model) {
        ToolCapability::Reliable => None,
        ToolCapability::Weak => Some(format!(
            "'{model}' is a weak tool-caller — small/quantized models often describe an action \
             or emit code instead of calling the tool, so results may under-report its real \
             ability. Prefer Qwen2.5+/Llama3.1+/Mistral for tool tasks."
        )),
        ToolCapability::Unknown => Some(format!(
            "Tool-calling reliability of '{model}' is unknown; if calls come back malformed, try \
             a known-reliable model (Qwen2.5+/Llama3.1+/Mistral)."
        )),
    }
}

/// Pick the tool calls to execute: the native `tool_calls` first; if empty, scan
/// the assistant `content` for a text-embedded call (JSON / `<tool_call>` XML) —
/// a common local-model failure where the call lands in prose instead.
pub fn select_calls(native_calls: &[NativeToolCall], content: &str) -> Vec<NativeToolCall> {
    if !native_calls.is_empty() {
        return native_calls.to_vec();
    }
    use crate::inference::eval::toolcall::parse::extract_calls;
    extract_calls(content)
        .unwrap_or_default()
        .into_iter()
        .map(|c| NativeToolCall { name: c.name, args: c.args })
        .collect()
}

/// The inert result of executing one model tool call against MCP.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolExecution {
    pub tool: String,
    pub is_error: bool,
    pub text: String,
}

/// Flatten a tool result's content blocks into inert text for the model.
pub fn flatten_content(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Image { .. } => "[image content]".to_string(),
            ContentBlock::Audio { .. } => "[audio content]".to_string(),
            ContentBlock::Other => "[unsupported content]".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Execute one model-emitted call against a connected server. The name may be
/// namespaced (`server::tool`); the bare tool name goes on the wire. Rechecks
/// `is_error` — a "successful" protocol result can still be a tool failure.
pub async fn execute_call(client: &McpClient, call: &NativeToolCall) -> AppResult<ToolExecution> {
    let tool = split_namespaced(&call.name).map(|(_, t)| t).unwrap_or(call.name.as_str());
    let result = client.call_tool(tool, call.args.clone()).await?;
    Ok(ToolExecution { tool: tool.to_string(), is_error: result.is_error(), text: flatten_content(&result) })
}

/// Call the model for ONE turn on the given backend, returning the shared
/// `ChatResult`. The single dispatch point — Ollama `/api/chat`, llama.cpp `/v1`;
/// MLX/remote have no native tool wire here.
pub async fn chat(
    backend: BackendKind,
    endpoint: &str,
    model: &str,
    system: &str,
    user: &str,
    tools: &Value,
    options: Option<GenerateOptions>,
) -> AppResult<ChatResult> {
    match backend {
        BackendKind::Ollama => {
            ollama_chat::chat_with_tools(endpoint, model, system, user, tools, options, Some(false)).await
        }
        BackendKind::LlamaCpp => {
            llama_chat::chat_with_tools(endpoint, model, system, user, tools, options).await
        }
        other => Err(AppError::Inference(format!(
            "{other:?} has no native tool-calling for MCP — select Ollama or llama.cpp"
        ))),
    }
}

/// The outcome of a single-turn bridge exchange.
pub struct SingleTurn {
    pub content: String,
    pub calls: Vec<NativeToolCall>,
    pub executions: Vec<ToolExecution>,
    pub warning: Option<String>,
}

/// One single-turn exchange: offer `tools`, take the model's call(s), execute
/// each against `client`. MLX (no native tool API) is rejected.
#[allow(clippy::too_many_arguments)]
pub async fn single_turn(
    backend: BackendKind,
    endpoint: &str,
    model: &str,
    system: &str,
    user: &str,
    tools: &[Tool],
    options: Option<GenerateOptions>,
    client: &McpClient,
) -> AppResult<SingleTurn> {
    let native = mcp_tools_to_native(tools);
    let result = chat(backend, endpoint, model, system, user, &native, options).await?;
    let calls = select_calls(&result.tool_calls, &result.content);
    let mut executions = Vec::with_capacity(calls.len());
    for c in &calls {
        executions.push(execute_call(client, c).await?);
    }
    Ok(SingleTurn { content: result.content, calls, executions, warning: capability_warning(model) })
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;

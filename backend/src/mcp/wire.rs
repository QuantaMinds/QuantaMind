//! JSON-RPC 2.0 + MCP wire types, modeled from `fixtures/mcp/*` (real captured
//! bytes), not the spec. Two rules the fixtures pin and the type system enforces:
//!
//! 1. **`result` xor `error`** — a [`Response`] carries exactly one, via an
//!    untagged [`ResponsePayload`]. A *tool* failure is a different thing again:
//!    an in-band [`CallToolResult`] with `is_error == true`.
//! 2. **Tolerate unknown fields** — no `deny_unknown_fields` anywhere. Real
//!    servers emit fields we don't model (`execution`, `$schema`, future keys);
//!    a strict parser would drop every tool (a documented real-world bug).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The only JSON-RPC version this client speaks.
pub const JSONRPC_VERSION: &str = "2.0";

/// Protocol revision this client offers in `initialize`. The server echoes it
/// on a match or counter-offers its own (handled in the Phase 2 client).
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Standard JSON-RPC error codes (protocol channel — distinct from an in-band
/// tool error, which is a `result` with `isError:true`).
pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

/// MCP method names this client uses.
pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const INITIALIZED: &str = "notifications/initialized";
    pub const TOOLS_LIST: &str = "tools/list";
    pub const TOOLS_CALL: &str = "tools/call";
    pub const TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";
}

/// A JSON-RPC id: number OR string per spec. We allocate integers, but a server
/// echoes whatever it was sent and may drive its own requests with string ids,
/// so both parse. `Eq + Hash` so it can key the Phase 2 correlation map.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    Num(i64),
    Str(String),
}

/// A JSON-RPC request (has an `id`, expects a response).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// A `2.0` request with the given id/method/params.
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Request { jsonrpc: JSONRPC_VERSION.to_string(), id, method: method.into(), params }
    }
}

/// A JSON-RPC notification: a method with **no** `id`, never answered. Sending
/// an `id` here (or answering one) is the `-32600` bug in the error catalog.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Notification { jsonrpc: JSONRPC_VERSION.to_string(), method: method.into(), params }
    }
}

/// A JSON-RPC response: `jsonrpc` + `id` + exactly one of `result` / `error`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(flatten)]
    pub payload: ResponsePayload,
}

/// `result` xor `error` — the type makes "both" and "neither" unrepresentable.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum ResponsePayload {
    Success { result: Value },
    Failure { error: JsonRpcError },
}

impl Response {
    /// The success payload, or the protocol error. Note this is the *protocol*
    /// channel only — a `tools/call` that succeeds here can still carry an
    /// in-band tool error (`CallToolResult::is_error`).
    pub fn result(&self) -> Result<&Value, &JsonRpcError> {
        match &self.payload {
            ResponsePayload::Success { result } => Ok(result),
            ResponsePayload::Failure { error } => Err(error),
        }
    }
}

/// A JSON-RPC error object (protocol channel).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

/// `name`/`version` of a client or server (`clientInfo` / `serverInfo`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Implementation {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// What the client advertises. Kept as tolerant `Option<Value>` blobs — we do
/// not gate behavior on our *own* capabilities, so their inner shape is inert.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

/// What the server advertises. Every field optional — presence is the signal
/// the client keys off (e.g. only call `tools/list` if `tools.is_some()`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completions: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

impl ServerCapabilities {
    /// True if the server advertised the `tools` capability at all.
    pub fn has_tools(&self) -> bool {
        self.tools.is_some()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Free text the server offers; treat as UNTRUSTED (may carry injection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

// ---------------------------------------------------------------------------
// tools/list + tools/call
// ---------------------------------------------------------------------------

/// Behavior hints a server attaches to a tool. **Untrusted** — a poisoned
/// server can set `readOnlyHint:true` on a destructive tool. The Phase 6
/// approval gate only honors these for a hash-pinned, user-trusted server.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ToolAnnotations {
    #[serde(default, rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// One tool a server exposes. `input_schema` is the JSON-Schema for arguments
/// (draft-07 in the reference server); kept as an opaque `Value`, mapped to the
/// the server `tools` shape in Phase 5. `description` is UNTRUSTED (tool poisoning).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(default, rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ToolsListResult {
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default, rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// The result of `tools/call`. Reaches the client on the *success* protocol
/// channel even when the tool itself failed — hence `is_error` must be
/// rechecked on every call (the single most-violated MCP assumption).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct CallToolResult {
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    /// Absent means false. Kept `Option` so "absent" and "explicit false" are
    /// distinguishable on the wire; read it through [`CallToolResult::is_error`].
    #[serde(default, rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

impl CallToolResult {
    /// Did the tool report failure in-band? Absent `isError` == false.
    pub fn is_error(&self) -> bool {
        self.is_error.unwrap_or(false)
    }
}

/// A typed content block. Tagged by `type`; unknown/newer types fall to
/// [`ContentBlock::Other`] so a single exotic block can never drop a result.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { data: String, #[serde(rename = "mimeType")] mime_type: String },
    Audio { data: String, #[serde(rename = "mimeType")] mime_type: String },
    #[serde(other)]
    Other,
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

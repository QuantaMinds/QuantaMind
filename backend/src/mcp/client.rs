//! Minimal MCP client over [`McpTransport`]: the handshake plus `tools/list` and
//! `tools/call`. Keeps only what a tool-using session needs — resources,
//! prompts, and sampling are out of scope.

use crate::errors::{AppError, AppResult};
use crate::mcp::transport::{McpTransport, DEFAULT_REQUEST_TIMEOUT};
use crate::mcp::wire::{
    method, CallToolParams, CallToolResult, ClientCapabilities, Implementation, InitializeParams,
    InitializeResult, ServerCapabilities, ToolsListResult, LATEST_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use std::time::Duration;

/// A connected, initialized MCP session.
pub struct McpClient {
    transport: McpTransport,
    protocol_version: String,
    server_info: Implementation,
    server_caps: ServerCapabilities,
}

impl McpClient {
    /// Spawn `program args…` and complete `initialize → initialized` with the
    /// default init window.
    pub async fn connect(
        program: &str,
        args: &[String],
        client_name: &str,
        client_version: &str,
    ) -> AppResult<McpClient> {
        Self::connect_with_timeout(program, args, client_name, client_version, DEFAULT_REQUEST_TIMEOUT)
            .await
    }

    /// Like [`connect`], but the caller sets how long to wait for `initialize` —
    /// MCP has no `/health`, so readiness IS the initialize response. A slow
    /// server gets a generous window; a timeout fails loud (never silently drops
    /// the server's tools).
    pub async fn connect_with_timeout(
        program: &str,
        args: &[String],
        client_name: &str,
        client_version: &str,
        init_timeout: Duration,
    ) -> AppResult<McpClient> {
        let transport = McpTransport::spawn(program, args)?;
        Self::handshake(transport, client_name, client_version, init_timeout).await
    }

    async fn handshake(
        transport: McpTransport,
        name: &str,
        version: &str,
        init_timeout: Duration,
    ) -> AppResult<McpClient> {
        let params = InitializeParams {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: name.to_string(),
                version: version.to_string(),
                title: None,
            },
        };
        let resp = transport
            .request_timeout(method::INITIALIZE, Some(serde_json::to_value(&params)?), init_timeout)
            .await?;
        let result: InitializeResult = match resp.result() {
            Ok(v) => serde_json::from_value(v.clone())
                .map_err(|e| AppError::Inference(format!("bad initialize result: {e}")))?,
            // The spec's -32602 "Unsupported protocol version" lands here; a
            // version counter-offer instead arrives on the success arm (accepted).
            Err(e) => {
                let extra = e.data.as_ref().map(|d| format!("; {d}")).unwrap_or_default();
                return Err(AppError::Inference(format!(
                    "MCP initialize rejected (code {}): {}{extra}",
                    e.code, e.message
                )));
            }
        };
        // The mandatory post-initialize notification (no id, never answered).
        transport.notify(method::INITIALIZED, None)?;

        Ok(McpClient {
            protocol_version: result.protocol_version,
            server_info: result.server_info,
            server_caps: result.capabilities,
            transport,
        })
    }

    pub fn server_info(&self) -> &Implementation {
        &self.server_info
    }
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.server_caps
    }
    /// Read-only access to the underlying transport (for methods this thin
    /// client doesn't wrap, and for tests).
    pub fn transport(&self) -> &McpTransport {
        &self.transport
    }

    /// List the server's tools. Refuses if the server never advertised the
    /// `tools` capability — probing an unadvertised method is the `-32601`
    /// failure in the error catalog.
    pub async fn list_tools(&self) -> AppResult<ToolsListResult> {
        if !self.server_caps.has_tools() {
            return Err(AppError::Inference(format!(
                "MCP server '{}' did not advertise the tools capability",
                self.server_info.name
            )));
        }
        let resp = self.transport.request(method::TOOLS_LIST, Some(json!({}))).await?;
        match resp.result() {
            Ok(v) => serde_json::from_value(v.clone())
                .map_err(|e| AppError::Inference(format!("bad tools/list result: {e}"))),
            Err(e) => Err(AppError::Inference(format!("tools/list error {}: {}", e.code, e.message))),
        }
    }

    /// Call a tool. A successful protocol result may still be a *tool* failure —
    /// the caller must recheck [`CallToolResult::is_error`].
    pub async fn call_tool(&self, name: &str, arguments: Value) -> AppResult<CallToolResult> {
        let params = CallToolParams { name: name.to_string(), arguments: Some(arguments) };
        let resp = self.transport.request(method::TOOLS_CALL, Some(serde_json::to_value(&params)?)).await?;
        match resp.result() {
            Ok(v) => serde_json::from_value(v.clone())
                .map_err(|e| AppError::Inference(format!("bad tools/call result: {e}"))),
            Err(e) => {
                Err(AppError::Inference(format!("tools/call protocol error {}: {}", e.code, e.message)))
            }
        }
    }

    /// Best-effort terminate the server (P3 hardens teardown/reaping).
    pub fn kill(&self) {
        self.transport.kill();
    }
}

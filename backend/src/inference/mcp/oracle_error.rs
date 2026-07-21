//! Track A — the **attribution oracle**. Whose fault was a tool-call failure:
//! the **model**, the **config**, or the **server**? MCP's two error channels are
//! the cheap, answer-key-free signal (see `docs/mcp/methodology.md`):
//!
//! - a client-side schema failure (hallucinated tool / bad args) → **model**;
//! - a top-level JSON-RPC `error` → protocol/**config**;
//! - a `result` with `isError:true` → tool/**server**.
//!
//! Coarse by design — finer automated attribution is a ~50%-accurate research
//! problem. Note the verified subtlety: an unknown *tool* is `-32602` (not
//! `-32601`, which is an unknown *method*), and `-32602` is itself model-vs-config
//! ambiguous — we only reach the server with it once our own schema check passed,
//! which tilts it to config.

use crate::inference::mcp::oracle_schema::CallCheck;
use crate::mcp::wire::{code, CallToolResult, Response, ResponsePayload};
use serde::Serialize;

/// Three-way fault attribution (plus `Success`, and `Blocked` for a schema-valid call
/// the deny-by-default approval gate refused to execute — never a fault of any party).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    Success,
    Model,
    Config,
    Server,
    /// The call was well-formed but NOT executed — the approval gate denied it
    /// (real tools default to deny; the user hasn't opted into execution). Counts as
    /// schema-valid, never as a success or a fault.
    Blocked,
}

/// What came back on the wire for a `tools/call`.
#[derive(Debug, Clone, PartialEq)]
pub enum WireOutcome {
    /// A `result` arrived; `is_error` is its in-band tool-error flag.
    Ok { is_error: bool },
    /// A top-level JSON-RPC `error`.
    ProtocolError { code: i64 },
    /// The connection died / timed out before a reply.
    Transport,
}

/// Classify a raw `tools/call` response into a wire outcome.
pub fn wire_outcome(resp: &Response) -> WireOutcome {
    match &resp.payload {
        ResponsePayload::Success { result } => {
            let is_error = serde_json::from_value::<CallToolResult>(result.clone())
                .map(|r| r.is_error())
                .unwrap_or(false);
            WireOutcome::Ok { is_error }
        }
        ResponsePayload::Failure { error } => WireOutcome::ProtocolError { code: error.code },
    }
}

/// Attribute a tool call from the client-side schema check + the wire outcome.
pub fn attribute(schema: &CallCheck, wire: &WireOutcome) -> Attribution {
    // A malformed / hallucinated call is the model's fault — caught client-side
    // before (or as) it hits the server.
    if !schema.is_valid() {
        return Attribution::Model;
    }
    match wire {
        WireOutcome::Ok { is_error: false } => Attribution::Success,
        // The tool ran and failed. Coarsely the tool/server layer (a model-
        // supplied bad VALUE also lands here — cheaply inseparable).
        WireOutcome::Ok { is_error: true } => Attribution::Server,
        WireOutcome::ProtocolError { code } => attribute_protocol(*code),
        WireOutcome::Transport => Attribution::Server,
    }
}

fn attribute_protocol(c: i64) -> Attribution {
    match c {
        // Unknown METHOD: the server doesn't implement it → protocol/config.
        code::METHOD_NOT_FOUND => Attribution::Config,
        // Invalid params despite our schema check passing → the server's real
        // schema is stricter than advertised (config/schema mismatch).
        code::INVALID_PARAMS => Attribution::Config,
        // Internal / server-defined → server.
        _ => Attribution::Server,
    }
}

#[cfg(test)]
#[path = "oracle_error_tests.rs"]
mod tests;

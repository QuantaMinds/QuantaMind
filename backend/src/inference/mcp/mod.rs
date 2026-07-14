//! MCP as an inference capability: bridging a model's tool calls to a real MCP
//! server. Depends on the `crate::mcp` protocol client (the natural layering —
//! inference uses the client; the client stays free of inference).

pub mod agent;
pub mod bridge;
pub mod gate;
pub mod oracle_error;
pub mod oracle_schema;

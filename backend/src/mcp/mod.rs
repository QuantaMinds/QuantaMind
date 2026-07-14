//! Model Context Protocol (MCP) client.
//!
//! The first runtime path in QuantaMind where a model call executes a real,
//! side-effecting tool (everything under `inference::eval` only *scores* tool
//! emission against mocked results). Hand-rolled JSON-RPC 2.0 over stdio — no
//! external MCP SDK, so the trust boundary for untrusted server output stays
//! fully auditable.
//!
//! Types live in [`wire`], modeled from real captured bytes in `fixtures/mcp/`,
//! not the spec (see `docs/mcp/`).

pub mod client;
pub mod transport;
pub mod wire;

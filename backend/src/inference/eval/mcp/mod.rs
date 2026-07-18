//! Track B — MCP as a controlled test *environment*. We seed a fresh sandbox we
//! own, point a REAL MCP server at only that sandbox, let a driver execute real
//! tool calls, then grade the sandbox's **end-state** (not the model's words) and
//! tear it down. Because we built the seed, the answer key is knowable even
//! though the tool is real (the τ-bench discipline; see `docs/mcp/methodology.md`).

pub mod oracle_db;
pub mod oracle_fs;
pub mod score;
pub mod validate;
pub mod world;

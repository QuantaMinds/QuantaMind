//! Tauri command surface for the MCP server registry + connection probing.

#[cfg(feature = "gui")]
pub mod mcp_cmd;
#[cfg(feature = "gui")]
pub mod run_cmd;
pub mod task_cmd;

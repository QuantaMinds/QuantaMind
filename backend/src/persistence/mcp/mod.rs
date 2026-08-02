//! On-disk MCP server registry (`mcp_servers.yaml`). Its own subfolder because
//! `persistence/` is at the folder-taxonomy limit (mirrors `persistence/jobs/`).
//! Distinct from the in-memory `crate::mcp::registry` of
//! live connections.

pub mod servers;

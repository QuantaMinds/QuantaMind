//! The MCP server registry persisted to `mcp_servers.yaml`.
//!
//! Stores only non-secret config. Secret env-var VALUES never touch this file —
//! only `env_keys` (their names) are stored; the values live in the OS keychain
//! (`SecureSecrets`), keyed by `(id, env_key)`. `roots` are canonicalized
//! (symlinks resolved) at use, never prefix-matched — the EscapeRoute-safe
//! boundary.

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

/// One configured MCP server. `command` is an executable/launcher (e.g. `npx`),
/// never a shell string — args are passed as a real argv, so there is no shell
/// to inject into.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct McpServerConfig {
    /// Stable id; unique in the registry and the namespace for the server's tools.
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Names (not values) of env vars whose values live in the keychain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    /// Directories a filesystem-style server is scoped to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl McpServerConfig {
    /// Canonicalize each declared root: resolve symlinks and require an existing
    /// directory. This is the confinement boundary — callers compare against the
    /// canonical path, never the raw string (which is how EscapeRoute
    /// CVE-2025-53109/53110 broke: prefix-match + symlink escape).
    pub fn canonical_roots(&self) -> AppResult<Vec<PathBuf>> {
        self.roots
            .iter()
            .map(|r| {
                let p = Path::new(r)
                    .canonicalize()
                    .map_err(|e| AppError::Io(format!("mcp root '{r}': {e}")))?;
                if !p.is_dir() {
                    return Err(AppError::Validation(format!("mcp root '{r}' is not a directory")));
                }
                Ok(p)
            })
            .collect()
    }
}

/// The whole registry.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct McpRegistry {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl McpRegistry {
    pub fn get(&self, id: &str) -> Option<&McpServerConfig> {
        self.servers.iter().find(|s| s.id == id)
    }
}

/// Reject a structurally invalid registry: empty/duplicate ids, empty command.
pub fn validate(reg: &McpRegistry) -> AppResult<()> {
    let mut seen = HashSet::new();
    for s in &reg.servers {
        if s.id.trim().is_empty() {
            return Err(AppError::Validation("mcp server has an empty id".into()));
        }
        if !seen.insert(s.id.as_str()) {
            return Err(AppError::Validation(format!("duplicate mcp server id '{}'", s.id)));
        }
        if s.command.trim().is_empty() {
            return Err(AppError::Validation(format!("mcp server '{}' has an empty command", s.id)));
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> AppResult<McpRegistry> {
    if !path.exists() {
        return Ok(McpRegistry::default());
    }
    let content = std::fs::read_to_string(path).map_err(|e| AppError::Io(e.to_string()))?;
    if content.trim().is_empty() {
        return Ok(McpRegistry::default());
    }
    let reg: McpRegistry = serde_yaml::from_str(&content).map_err(|e| AppError::Internal(e.to_string()))?;
    validate(&reg)?;
    Ok(reg)
}

pub fn save(path: &Path, reg: &McpRegistry) -> AppResult<()> {
    validate(reg)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    let yaml = serde_yaml::to_string(reg).map_err(|e| AppError::Internal(e.to_string()))?;
    std::fs::write(path, yaml).map_err(|e| AppError::Io(e.to_string()))
}

#[cfg(test)]
#[path = "servers_tests.rs"]
mod tests;

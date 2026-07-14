//! In-memory registry of live MCP connections, plus the security helpers that
//! make holding multiple/untrusted servers safe: per-server tool **namespacing**
//! (anti cross-server shadowing) and tool-definition **hash-pinning** (rug-pull
//! detection). Distinct from the on-disk `crate::persistence::mcp::servers`.

use crate::mcp::client::McpClient;
use crate::mcp::wire::Tool;
use crate::sync::MutexExt;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

/// Separator between a server id and a tool name in a namespaced tool name.
pub const NS_SEP: &str = "::";

/// Namespace a tool under its server, so two servers both exposing `read_file`
/// can never shadow one another when their tools are offered to a model together.
pub fn namespaced(server_id: &str, tool: &str) -> String {
    format!("{server_id}{NS_SEP}{tool}")
}

/// Split a namespaced name back into `(server_id, tool)`. `None` if not namespaced.
pub fn split_namespaced(name: &str) -> Option<(&str, &str)> {
    name.split_once(NS_SEP)
}

/// A stable fingerprint of a tool's SECURITY-RELEVANT definition (name +
/// description + input schema). A change here between connects is a rug-pull:
/// the server earned approval with one definition and swapped in another.
pub fn tool_fingerprint(tool: &Tool) -> String {
    let mut h = Sha256::new();
    h.update(tool.name.as_bytes());
    h.update([0]);
    h.update(tool.description.as_deref().unwrap_or("").as_bytes());
    h.update([0]);
    // serde_json's Map is a BTreeMap (sorted keys) without the `preserve_order`
    // feature, so this serialization is deterministic across runs.
    h.update(serde_json::to_string(&tool.input_schema).unwrap_or_default().as_bytes());
    format!("{:x}", h.finalize())
}

/// Tool fingerprints captured at connect time, to diff on reconnect /
/// `notifications/tools/list_changed`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PinnedTools {
    fps: HashMap<String, String>,
}

impl PinnedTools {
    pub fn from_tools(tools: &[Tool]) -> Self {
        PinnedTools { fps: tools.iter().map(|t| (t.name.clone(), tool_fingerprint(t))).collect() }
    }

    /// Compare current tools against the pinned set.
    pub fn diff(&self, current: &[Tool]) -> ToolDiff {
        let mut d = ToolDiff::default();
        let cur: HashMap<&str, String> =
            current.iter().map(|t| (t.name.as_str(), tool_fingerprint(t))).collect();
        for (name, fp) in &cur {
            match self.fps.get(*name) {
                None => d.added.push((*name).to_string()),
                Some(old) if old != fp => d.changed.push((*name).to_string()),
                Some(_) => {}
            }
        }
        for name in self.fps.keys() {
            if !cur.contains_key(name.as_str()) {
                d.removed.push(name.clone());
            }
        }
        d
    }
}

/// The result of comparing live tools to the pinned set.
#[derive(Debug, Default, PartialEq)]
pub struct ToolDiff {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl ToolDiff {
    /// A rug-pull: a previously-approved tool changed its definition or vanished.
    /// A newly-*added* tool is not a rug-pull — it needs approval, not alarm.
    pub fn is_rug_pull(&self) -> bool {
        !self.changed.is_empty() || !self.removed.is_empty()
    }
}

/// One live, initialized connection + the tools pinned when it connected.
pub struct ConnectedServer {
    pub client: McpClient,
    pub pins: PinnedTools,
}

/// Tauri-managed set of live MCP connections, keyed by server id. Every
/// connection is killed on reap (`kill_all`) and on drop.
#[derive(Default)]
pub struct McpServerState {
    inner: Mutex<HashMap<String, ConnectedServer>>,
}

impl McpServerState {
    /// Store a connection, killing any previous one under the same id.
    pub fn insert(&self, id: impl Into<String>, server: ConnectedServer) {
        if let Some(prev) = self.inner.lock_recover().insert(id.into(), server) {
            prev.client.kill();
        }
    }
    pub fn is_connected(&self, id: &str) -> bool {
        self.inner.lock_recover().contains_key(id)
    }
    pub fn remove_and_kill(&self, id: &str) {
        if let Some(s) = self.inner.lock_recover().remove(id) {
            s.client.kill();
        }
    }
    /// Kill and forget every connection. Idempotent — the reap hook + `Drop`.
    pub fn kill_all(&self) {
        for (_, s) in self.inner.lock_recover().drain() {
            s.client.kill();
        }
    }
}

impl Drop for McpServerState {
    fn drop(&mut self) {
        self.kill_all();
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

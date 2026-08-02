//! Load a certify suite. Reuses the existing `McpTaskSpec` shape verbatim, so a
//! file authored in the desktop MCP builder runs under `qm certify` with no edits.

use crate::commands::mcp::task_cmd::{negative_task_findings, to_mcp_spec, McpTaskSpec, TaskKind};
use crate::inference::eval::mcp::validate::static_world_findings;
use crate::inference::eval::mcp::world::McpSpec;
use std::collections::HashSet;

/// Cap on a suite file, matching the collection cap. A real enterprise fixture
/// belongs in a referenced file, not inline.
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct CertifyTask {
    pub id: String,
    pub goal: String,
    pub spec: McpSpec,
    pub k: u32,
    pub is_negative: bool,
}

/// Parse and validate a suite file. Every rejection here is a *load-time* error,
/// so an unusable suite never costs a single agent invocation.
pub fn load(path: &std::path::Path) -> Result<Vec<CertifyTask>, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        format!("cannot read the suite file: {}", crate::redact::redact_path(&e.to_string()))
    })?;
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "suite file is {} bytes, over the {MAX_BYTES}-byte cap — reference large fixtures \
             from a file instead of inlining them",
            bytes.len()
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| "suite file is not valid UTF-8".to_string())?;
    let specs: Vec<McpTaskSpec> = serde_json::from_str(&text).map_err(|e| {
        format!(
            "suite must be a JSON array of world tasks ({{name, instruction, world, oracle}}): {e}"
        )
    })?;
    if specs.is_empty() {
        return Err("the suite has no tasks".into());
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for s in &specs {
        // Duplicate ids would produce ambiguous report rows and JUnit testcases.
        if !seen.insert(s.name.as_str()) {
            return Err(format!("duplicate task name '{}' — names identify rows and must be unique", s.name));
        }
        if s.k == 0 {
            return Err(format!("task '{}' has k=0 — a task that never runs cannot pass", s.name));
        }
        let spec = to_mcp_spec(s);
        // The same authoring gate the GUI import and `qm validate` apply, so the
        // surfaces can never drift: vacuous/contradictory oracles, and paths or
        // queries that would make a task silently unfailable.
        let mut findings = static_world_findings(&spec);
        findings.extend(negative_task_findings(s));
        if !findings.is_empty() {
            return Err(format!("task '{}': {}", s.name, findings.join("; ")));
        }
        out.push(CertifyTask {
            id: s.name.clone(),
            // Verbatim. No preamble: the agent under test is not ours to instruct.
            goal: s.instruction.clone(),
            spec,
            k: s.k,
            is_negative: s.kind == TaskKind::Negative,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "suite_tests.rs"]
mod tests;

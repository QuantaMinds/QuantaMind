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
    /// The task exactly as it appeared in the file. `--record` rewrites only the
    /// `oracle` on this, so the world (and any field this loader does not model)
    /// round-trips byte-for-byte instead of being re-serialised from our types.
    pub source: serde_json::Value,
}

/// Parse and validate a suite file. Every rejection here is a *load-time* error,
/// so an unusable suite never costs a single agent invocation.
pub fn load(path: &std::path::Path) -> Result<Vec<CertifyTask>, String> {
    load_with(path, false)
}

/// Load a suite that is about to be **recorded** rather than run.
///
/// Waives exactly one finding: "vacuous oracle". Recording exists because the
/// author does not have an answer key yet — demanding one first would defeat the
/// entire feature. Nothing else is relaxed: escaping oracle paths, mutating
/// oracle queries, duplicate names and `k=0` are still hard errors, because those
/// are wrong regardless of whether an oracle is present. The vacuity gate is
/// re-applied in full the moment the recorded suite is run for real.
pub fn load_for_recording(path: &std::path::Path) -> Result<Vec<CertifyTask>, String> {
    load_with(path, true)
}

fn load_with(path: &std::path::Path, recording: bool) -> Result<Vec<CertifyTask>, String> {
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
    // Accept a bare array, or `{ "tasks": [...] }` — the shape `--record` writes,
    // so a recorded suite replays without hand-editing. Anything else is an error
    // naming both.
    let raw: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("suite is not valid JSON: {e}"))?;
    let arr = match &raw {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => match o.get("tasks") {
            Some(serde_json::Value::Array(a)) => a.clone(),
            _ => {
                return Err(
                    "suite must be a JSON array of world tasks, or an object with a `tasks` array \
                     ({name, instruction, world, oracle})"
                        .into(),
                )
            }
        },
        _ => {
            return Err(
                "suite must be a JSON array of world tasks, or an object with a `tasks` array \
                 ({name, instruction, world, oracle})"
                    .into(),
            )
        }
    };
    let specs: Vec<McpTaskSpec> = serde_json::from_value(serde_json::Value::Array(arr.clone()))
        .map_err(|e| {
            format!(
                "suite must be a JSON array of world tasks ({{name, instruction, world, oracle}}): {e}"
            )
        })?;
    if specs.is_empty() {
        return Err("the suite has no tasks".into());
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for (i, s) in specs.iter().enumerate() {
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
        if recording {
            // The ONE waiver — see `load_for_recording`.
            findings.retain(|f| !f.contains("vacuous oracle"));
        }
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
            source: arr.get(i).cloned().unwrap_or(serde_json::Value::Null),
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "suite_tests.rs"]
mod tests;

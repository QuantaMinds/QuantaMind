use crate::inference::eval::harness::grade::grade;
use crate::inference::eval::harness::workspace::Workspace;
use crate::inference::eval::mcp::world::McpSpec;

/// Does this task actually test anything?
///
/// Seeds a workspace, runs **zero** commands, and grades. A task a do-nothing
/// agent passes proves nothing — it is green regardless of what the agent does,
/// so it would silently inflate every future verdict. Returns `true` when the
/// task is discriminating (i.e. doing nothing FAILS it).
///
/// This is the npx-free twin of `mcp::validate::validate_world_live`, which has
/// the same semantics but builds a real `McpWorld` and therefore spawns
/// `npx @modelcontextprotocol/server-filesystem`. Nothing here spawns anything,
/// so it costs milliseconds and needs no Node — which is why it can run
/// **unconditionally** before every certify run rather than being opt-in.
pub fn is_discriminating(spec: &McpSpec, task_id: &str, goal: &str) -> Result<bool, String> {
    let ws = Workspace::seed(spec, task_id, goal)
        .map_err(|e| crate::redact::redact_path(&e.to_string()))?;
    // Zero agent actions. Anything the oracle finds here, it would find on an
    // empty run.
    let do_nothing_passes = grade(spec, &ws).is_empty();
    Ok(!do_nothing_passes)
}

#[cfg(test)]
#[path = "discriminate_tests.rs"]
mod tests;

use crate::inference::eval::harness::workspace::Workspace;
use crate::inference::eval::mcp::world::McpSpec;

/// Grade the real end state of a workspace.
///
/// Returns the oracle's own failure strings rather than a bool, because a verdict
/// that says only "failed" is useless in CI — the user needs to know *which*
/// assertion broke. `McpWorld::grade` collapses to a bool for the model path; this
/// mode keeps the detail.
///
/// Nothing here reads the agent's output. The grade is the world, not the words:
/// a plausible-sounding transcript with an untouched filesystem fails, which is
/// the entire reason this mode exists.
pub fn grade(spec: &McpSpec, ws: &Workspace) -> Vec<String> {
    match spec {
        McpSpec::Fs { oracle, .. } => oracle.grade(ws.root()).failures,
        McpSpec::Db { oracle, .. } => match ws.db() {
            Some(db) => oracle.grade(db).failures,
            // Can't happen for a well-formed Db world (the workspace always
            // materializes the file), but say so honestly rather than reporting a
            // clean pass on a world we never actually looked at.
            None => vec!["db world has no database file — the workspace was not seeded".into()],
        },
    }
}

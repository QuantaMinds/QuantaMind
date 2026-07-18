//! `qm validate` — prove a collection is a RELIABLE test before any model runs it:
//! structural schema → deep sandbox validation (reachable by a perfect agent,
//! failed by a do-nothing agent) → world checks for MCP tasks (static
//! contradiction/vacuity + LIVE spawn + do-nothing-against-the-real-oracle).
//! The same pipeline gates `qm run`/`test` on uploaded files — this command is its
//! detailed-report form.

use crate::cli::run::{load_collection, CollectionError};
use crate::errors::AppResult;
use crate::inference::eval::agentic::v2::oracle::{validate_collection_deep, CollectionValidation};
use crate::inference::eval::mcp::validate::{merge_world_checks, world_deps_missing};
use crate::inference::eval::mcp::world::McpSpec;
use crate::inference::eval::toolcall::tasks::ToolTask;

pub enum ValidateOutcome {
    UnknownCollection { id: String },
    BadFile { path: String, reason: String },
    /// Worlds exist but the machine can't live-check them (npx/sqlite3 missing) —
    /// inconclusive, exit 11, with the exact install fix.
    DepsMissing { fix: String, validation: CollectionValidation },
    Done(CollectionValidation),
}

/// Exit for a completed validation: 0 all valid · 10 warnings only · 20 invalid.
pub fn validate_exit(v: &CollectionValidation) -> i32 {
    if !v.ok {
        20
    } else if v.tasks.iter().any(|t| !t.semantic_warnings.is_empty()) {
        10
    } else {
        0
    }
}

fn world_specs(tasks: &[ToolTask]) -> Vec<&McpSpec> {
    tasks.iter().filter_map(|t| t.agentic.as_ref().and_then(|a| a.mcp.as_ref())).collect()
}

/// The full validation pipeline over a collection spec (built-in id or file).
pub async fn run_validate(collection: &str, live: bool) -> AppResult<ValidateOutcome> {
    let tasks = match load_collection(collection) {
        Ok(t) => t.tasks,
        Err(CollectionError::UnknownBuiltin) => return Ok(ValidateOutcome::UnknownCollection { id: collection.into() }),
        Err(CollectionError::BadFile(reason)) => return Ok(ValidateOutcome::BadFile { path: collection.into(), reason }),
    };
    let mut v = validate_collection_deep(&tasks).await;
    let deps = world_deps_missing(&world_specs(&tasks));
    merge_world_checks(&mut v, &tasks, live && deps.is_none()).await;
    match deps {
        // Static findings may already have failed it — a hard fail outranks inconclusive.
        Some(fix) if v.ok => Ok(ValidateOutcome::DepsMissing { fix, validation: v }),
        _ => Ok(ValidateOutcome::Done(v)),
    }
}

/// Human render: one line per task (`reachable · discriminating · detail`), then
/// its ✗ findings / ! warnings, then the verdict line.
pub fn render_validation(v: &CollectionValidation) -> String {
    let mut out = String::new();
    if let Some(e) = &v.structural_error {
        out.push_str(&format!("✗ structural: {e}\n"));
    }
    for t in &v.tasks {
        let disc = match t.discriminating {
            Some(true) => "discriminating ✓",
            Some(false) => "NOT discriminating ✗",
            None => "discrimination n/a",
        };
        out.push_str(&format!("{:<28} reachable: {:<13} {}", t.id, t.reachable, disc));
        if !t.detail.is_empty() {
            out.push_str(&format!("  — {}", t.detail));
        }
        out.push('\n');
        for f in &t.semantic {
            out.push_str(&format!("    ✗ {f}\n"));
        }
        for w in &t.semantic_warnings {
            out.push_str(&format!("    ! {w}\n"));
        }
    }
    out.push_str(if v.ok { "VALID: every task is a trustworthy test\n" } else { "INVALID: fix the ✗ findings before running — a broken answer key makes every pass^k a lie\n" });
    out
}

#[cfg(test)]
#[path = "validate_cli_tests.rs"]
mod tests;

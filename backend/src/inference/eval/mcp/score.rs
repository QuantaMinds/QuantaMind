//! Track B pass^k scoring: run a task against a FRESH controlled world k times,
//! drive real tools, grade the WORLD end-state each run, and report a
//! *reliability* verdict — ready only if every run passed (a single lucky pass
//! is not readiness). This is the τ-bench discipline on an MCP world.

use crate::errors::AppResult;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::{FsSeed, McpWorld};
use crate::inference::mcp::agent::{run_loop, McpExecutor, TurnDriver};
use crate::inference::mcp::gate::Decision;
use crate::mcp::wire::Tool;
use serde::Serialize;
use std::path::Path;

/// A Track B task: the `world` (seed) + `oracle` (answer key) that the plan's
/// task-format adds around the instruction. `mock_result` is gone — we grade the
/// real world, not a canned string.
pub struct McpTask {
    pub instruction: String,
    pub seed: FsSeed,
    pub oracle: FsOracle,
}

/// pass^k over a controlled world.
#[derive(Debug, Clone, Serialize)]
pub struct McpScore {
    pub k: usize,
    pub passes: usize,
    /// The oracle failures of each FAILED run (for the "why not ready" report).
    pub failures: Vec<Vec<String>>,
}

impl McpScore {
    pub fn pass_rate(&self) -> f64 {
        if self.k == 0 {
            0.0
        } else {
            self.passes as f64 / self.k as f64
        }
    }
    /// Strict pass^k: ready only if EVERY run passed.
    pub fn is_ready(&self) -> bool {
        self.k > 0 && self.passes == self.k
    }
}

/// Run `task` k times, a fresh world each run (the pass^k independence
/// precondition), driving it with a per-run driver from `make_driver`, then
/// grading the end-state. Controlled worlds are safe by isolation, so the gate
/// auto-approves.
pub async fn score_fs_task<D, MK>(
    task: &McpTask,
    mut make_driver: MK,
    k: usize,
    max_steps: usize,
) -> AppResult<McpScore>
where
    D: TurnDriver,
    MK: FnMut(&Path, &[Tool]) -> D,
{
    let mut passes = 0;
    let mut failures = Vec::new();
    for _ in 0..k {
        let world = McpWorld::filesystem(&task.seed).await?;
        let tools = world.client().list_tools().await?.tools;
        let mut driver = make_driver(world.root(), &tools);
        let exec = McpExecutor::new(world.client());
        // Controlled disposable world → auto-approve (isolation is the safety).
        let _ = run_loop(&mut driver, &exec, |_| Decision::Approve, max_steps).await?;
        // Grade the WORLD, not the model's words.
        let verdict = task.oracle.grade(world.root());
        if verdict.passed {
            passes += 1;
        } else {
            failures.push(verdict.failures);
        }
    }
    Ok(McpScore { k, passes, failures })
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;

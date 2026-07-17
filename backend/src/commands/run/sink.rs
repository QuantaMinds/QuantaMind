//! A minimal `BatchSink` for the CLI: streams run progress to STDERR (diagnostics),
//! keeping STDOUT clean for the report (so `qm run --json | jq` is never polluted).
//! The GUI's sink emits Tauri events; the CLI just prints.

use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::batch::{BatchSink, TaskOutcome};

pub struct CliSink {
    /// Task count (for context in the start line); the engine also passes a `total`.
    _total: usize,
}

impl CliSink {
    pub fn new(total: usize) -> Self {
        Self { _total: total }
    }
}

impl BatchSink for CliSink {
    fn task_started(&self, _model: &str, task_id: &str, index: usize, total: usize, _category: &str, _is_native: bool) {
        eprintln!("· [{}/{}] {task_id}", index + 1, total);
    }

    fn agentic_turn(&self, _model: &str, _task_id: &str, _step: &TrajectoryStep, _is_native: bool) {}

    fn task_done(&self, _model: &str, task_id: &str, outcome: &TaskOutcome, _is_native: bool) {
        // Only a per-task ERROR is worth a line here — pass/fail is the verdict's job,
        // not the progress stream's (and we never fabricate a pass badge, rule 7).
        if let TaskOutcome::Error { message } = outcome {
            eprintln!("  ✗ {task_id}: {message}");
        }
    }
}

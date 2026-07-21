//! A minimal `BatchSink` for the CLI: streams run progress to STDERR (diagnostics),
//! keeping STDOUT clean for the report (so `qm run --json | jq` is never polluted).
//! The GUI's sink emits Tauri events; the CLI prints — and, when `--costs` asked for
//! it, CAPTURES each turn (with the same step-END host-RSS sample the GUI sink takes)
//! so the run's per-task costs can be assembled afterwards.

use crate::commands::system::process_memory;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::eval::agentic::step::TrajectoryStep;
use crate::inference::eval::batch::{BatchSink, TaskOutcome};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub struct CliSink {
    /// Task count (for context in the start line); the engine also passes a `total`.
    _total: usize,
    /// `Some(backend)` = capture mode (`--costs`): steps accumulate per (task, pass)
    /// with a step-end RSS sample of that backend's local server — the SAME boundary
    /// sampling `TauriBatchSink` does. `None` = the old print-only sink, zero capture
    /// cost.
    capture: Option<BackendKind>,
    steps: Mutex<BTreeMap<(String, bool), Vec<TrajectoryStep>>>,
    outcomes: Mutex<BTreeMap<(String, bool), TaskOutcome>>,
}

impl CliSink {
    pub fn new(total: usize) -> Self {
        Self { _total: total, capture: None, steps: Mutex::new(BTreeMap::new()), outcomes: Mutex::new(BTreeMap::new()) }
    }

    /// Capture mode for `--costs`: remember every turn, RSS-stamped at step end.
    pub fn capturing(total: usize, backend: BackendKind) -> Self {
        Self { capture: Some(backend), ..Self::new(total) }
    }

    /// The captured (task, pass) → steps cells, for `costs::assemble`.
    pub fn captured_steps(&self) -> BTreeMap<(String, bool), Vec<TrajectoryStep>> {
        self.steps.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn captured_outcomes(&self) -> BTreeMap<(String, bool), TaskOutcome> {
        self.outcomes.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl BatchSink for CliSink {
    fn task_started(&self, _model: &str, task_id: &str, index: usize, total: usize, _category: &str, is_native: bool) {
        let pass = if is_native { "native" } else { "prompt" };
        eprintln!("· [{}/{}] {task_id} ({pass})", index + 1, total);
    }

    fn agentic_turn(&self, _model: &str, task_id: &str, step: &TrajectoryStep, is_native: bool) {
        let Some(backend) = self.capture else { return };
        // Step-END host sample: whole-process RSS of the local server (weights +
        // residue — never a per-task delta; see the field's contract on TrajectoryStep).
        let mut step = step.clone();
        if step.resident_bytes.is_none() {
            step.resident_bytes = process_memory::backend_rss(backend);
        }
        self.steps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry((task_id.to_string(), is_native))
            .or_default()
            .push(step);
    }

    fn task_done(&self, _model: &str, task_id: &str, outcome: &TaskOutcome, is_native: bool) {
        // Only a per-task ERROR is worth a line here — pass/fail is the verdict's job,
        // not the progress stream's (and we never fabricate a pass badge, rule 7). Redact
        // the message: a bubbled-up I/O error can embed an absolute path (rule 7f).
        if let TaskOutcome::Error { message, .. } = outcome {
            eprintln!("  ✗ {task_id}: {}", crate::redact::redact_path(message));
        }
        if self.capture.is_some() {
            self.outcomes
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert((task_id.to_string(), is_native), outcome.clone());
        }
    }
}

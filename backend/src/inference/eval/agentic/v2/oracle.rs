//! Answer-key validation for authored collections — a "does my task actually work?" check
//! the user runs BEFORE spending a real model on it. It proves each task is
//! (a) *reachable* — a perfect "oracle" agent that replays the checkpoint calls reaches the
//! end state — and (b) *discriminating* — a do-nothing agent FAILS it. A task that fails (a)
//! has a broken answer key (a typo'd checkpoint tool, a wildcard that never matches, a required
//! call with no mock); one that fails (b) is trivially passable and measures nothing. Both are
//! authoring bugs the user could otherwise mistake for "every model is bad at my task".
//!
//! This is the same oracle the build-time satisfiability test uses for the bundled collections,
//! lifted into a reusable path so a custom/imported collection gets the identical proof.

use crate::errors::AppResult;
use crate::inference::eval::agentic::build::sandbox_for;
use crate::inference::eval::agentic::model_turn::ModelTurn;
use crate::inference::eval::agentic::runner::run_once;
use crate::inference::eval::agentic::sandbox::EndStateRule;
use crate::inference::eval::agentic::spec::FaultInjection;
use crate::inference::eval::toolcall::tasks::{is_agentic, ToolTask};
use crate::inference::generate::generate_spec::GenerateSpec;
use crate::inference::generate::generate_stats::GenerateStats;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc::unbounded_channel;

/// A canned agent: emits `calls[i]` on turn `i`, then a no-op `{}` (no tool call) forever.
/// The whole `ModelTurn` seam — no backend, so validation needs no server or model.
struct Scripted {
    calls: Vec<String>,
    next: AtomicUsize,
}

impl ModelTurn for Scripted {
    async fn run(&self, _s: &GenerateSpec) -> AppResult<(String, GenerateStats)> {
        let i = self.next.fetch_add(1, Ordering::SeqCst);
        let body = self.calls.get(i).cloned().unwrap_or_else(|| "{}".into());
        Ok((body, GenerateStats { eval_count: Some(1), ..Default::default() }))
    }
}

/// Replace each `*…*` wildcard string with a concrete value that satisfies the glob (its
/// literal segments joined); everything else stays exact. Lets the oracle satisfy a wildcard
/// checkpoint the way a correct model would (a real value that matches the pattern).
fn concretize(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), concretize(x))).collect()),
        Value::String(s) if s.contains('*') => {
            let lit: String = s.split('*').filter(|p| !p.is_empty()).collect();
            Value::String(if lit.is_empty() { "x".into() } else { lit })
        }
        other => other.clone(),
    }
}

/// The oracle's call script for a checkpoint end-state: each checkpoint (concretized), repeated
/// enough times to clear any transient fault on its tool (the fault fires before the advance, so
/// the tool's FIRST occurrence needs `clears_after` extra retries). `None` when the end state is
/// not checkpoint-driven (`RequireEndState` stateful UI / `ExpectAbstainingText`), which this
/// path can't script from a transpiled `ToolTask`.
fn oracle_calls(task: &ToolTask) -> Option<Vec<String>> {
    let spec = task.agentic.as_ref()?;
    let cps = match &spec.end_state {
        EndStateRule::RequireAll(cps) | EndStateRule::RequireSequence(cps) => cps,
        _ => return None,
    };
    let mut calls = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for cp in cps {
        // Transient faults are keyed by tool NAME (a global counter), so only the tool's first
        // appearance needs the extra retries.
        let retries = if seen.insert(cp.tool.clone()) {
            spec.name_faults
                .iter()
                .find(|f| f.on_call == cp.tool)
                .map(|f| match f.fault {
                    FaultInjection::TransientError { clears_after, .. } => clears_after as usize,
                    FaultInjection::PersistentError { .. } => 0,
                })
                .unwrap_or(0)
        } else {
            0
        };
        let body = json!({ "name": cp.tool, "args": concretize(&cp.args) }).to_string();
        for _ in 0..=retries {
            calls.push(body.clone());
        }
    }
    Some(calls)
}

/// Per-task validation verdict, serialized to the UI. `reachable` is `"yes"` / `"no"` /
/// `"not_checkable"` (the last for stateful/abstain end-states this static path can't script —
/// the user must confirm those with a real run). `discriminating` is whether a do-nothing agent
/// correctly FAILS the task (`None` when not applicable). `detail` is the human explanation.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct TaskValidation {
    pub id: String,
    pub reachable: String,
    pub discriminating: Option<bool>,
    pub detail: String,
}

/// Whole-collection verdict. `ok` is true only when there is no structural error AND no task
/// is definitively broken (unreachable or non-discriminating). `structural_error` is `Some`
/// when the schema trust-boundary (`validate_tasks`) rejected the collection outright — then
/// `tasks` is empty (there was nothing well-formed to oracle-check).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct CollectionValidation {
    pub ok: bool,
    pub structural_error: Option<String>,
    pub tasks: Vec<TaskValidation>,
}

/// Oracle-check ONE (already structurally-valid) task: run the perfect replay agent and, for a
/// checkpoint task, a do-nothing agent, and report reachability + whether the task discriminates.
async fn validate_one(task: &ToolTask) -> TaskValidation {
    // Single-turn (non-agentic) tasks are graded by their `expected` call, not by running a
    // loop — structural validation already proved that field is present and consistent.
    if !is_agentic(&task.category) {
        return TaskValidation {
            id: task.id.clone(),
            reachable: "yes".into(),
            discriminating: None,
            detail: "Single-turn task — graded on its expected call; validated structurally.".into(),
        };
    }

    let Some(calls) = oracle_calls(task) else {
        return TaskValidation {
            id: task.id.clone(),
            reachable: "not_checkable".into(),
            discriminating: None,
            detail: "Stateful or abstain end-state — reachability can't be auto-derived; confirm with a real run.".into(),
        };
    };

    let (sandbox, cfg) = match sandbox_for(task) {
        Ok(v) => v,
        Err(e) => {
            return TaskValidation {
                id: task.id.clone(),
                reachable: "no".into(),
                discriminating: None,
                detail: format!("Could not build the task environment: {e}"),
            }
        }
    };

    // Oracle run: the perfect replay must reach the end state with no unknown-tool calls.
    let oracle = Scripted { calls, next: AtomicUsize::new(0) };
    let (tx, _rx) = unbounded_channel();
    let oracle_out = run_once(&oracle, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx).await;
    let reached = matches!(&oracle_out, Ok(o) if o.reached_end && o.unknown_tool_calls == 0 && o.failure.is_none());
    if !reached {
        let why = match &oracle_out {
            Ok(o) if o.unknown_tool_calls > 0 => "a checkpoint names a tool the environment doesn't mock".into(),
            Ok(o) => format!("the oracle ended without success ({:?})", o.failure),
            Err(e) => format!("the run errored ({e})"),
        };
        return TaskValidation {
            id: task.id.clone(),
            reachable: "no".into(),
            discriminating: None,
            detail: format!("Answer key not reachable — {why}. Check the checkpoint tool names, args, and wildcards."),
        };
    }

    // Discrimination floor: a do-nothing agent must FAIL, or the task is trivially passable.
    let lazy = Scripted { calls: vec![], next: AtomicUsize::new(0) };
    let (tx2, _rx2) = unbounded_channel();
    let lazy_out = run_once(&lazy, &sandbox, cfg.max_steps, cfg.max_recovery, 0, &tx2).await;
    let discriminates = matches!(&lazy_out, Ok(o) if !o.reached_end);
    TaskValidation {
        id: task.id.clone(),
        reachable: "yes".into(),
        discriminating: Some(discriminates),
        detail: if discriminates {
            "Reachable by the oracle and a do-nothing agent fails it — solvable and discriminating.".into()
        } else {
            "Reachable, BUT a do-nothing agent also passes — the task is trivially satisfiable and measures nothing.".into()
        },
    }
}

/// Deep-validate a collection: the structural trust boundary first, then the per-task oracle
/// proof. `ok` is false if the structure is rejected OR any task is unreachable / trivially
/// passable. Callers (the Tauri command) load the tasks (bundled, saved, or freshly parsed) and
/// hand them here — the one place both import and authoring get the same guarantee.
pub async fn validate_collection_deep(tasks: &[ToolTask]) -> CollectionValidation {
    if let Err(e) = crate::inference::eval::toolcall::tasks::validate_tasks(tasks) {
        return CollectionValidation { ok: false, structural_error: Some(e.to_string()), tasks: Vec::new() };
    }
    let mut out = Vec::with_capacity(tasks.len());
    let mut all_ok = true;
    for t in tasks {
        let v = validate_one(t).await;
        if v.reachable == "no" || v.discriminating == Some(false) {
            all_ok = false;
        }
        out.push(v);
    }
    CollectionValidation { ok: all_ok, structural_error: None, tasks: out }
}

#[cfg(test)]
#[path = "oracle_tests.rs"]
mod tests;

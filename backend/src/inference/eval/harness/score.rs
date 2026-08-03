use crate::inference::eval::harness::attempt::{
    ActuatorReport, ActuatorResult, Attempt, AttemptContext, AttemptStatus,
};
use crate::inference::eval::harness::grade::grade;
use crate::inference::eval::harness::workspace::Workspace;
use crate::inference::eval::mcp::world::McpSpec;

/// One task, graded k times.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: String,
    pub attempts: Vec<Attempt>,
    /// Strict pass^k: k requested, and every one of them passed.
    pub k: u32,
}

impl TaskResult {
    /// How many attempts passed outright.
    pub fn passes(&self) -> u32 {
        self.attempts.iter().filter(|a| a.status.is_pass()).count() as u32
    }

    /// **Strict pass^k** — every one of the k attempts passed.
    ///
    /// Not pass@k. A 70%-per-trial agent reads ~97% on pass@3 and ~34% on pass^3;
    /// the gap is the whole point. A deploy gate cares whether it works *every*
    /// time, so one flaky attempt fails the task.
    pub fn is_strict_pass(&self) -> bool {
        self.k > 0 && self.passes() == self.k && !self.inconclusive()
    }

    /// True if any attempt could not be measured. Such a run can never be green —
    /// an incomplete measurement is not evidence of correctness.
    pub fn inconclusive(&self) -> bool {
        self.attempts.iter().any(|a| a.status.is_inconclusive())
    }

    /// True when *every* attempt failed for a reason we could actually observe.
    /// Distinguishes "broken" from "flaky" in the verdict rollup.
    pub fn failed_every_attempt(&self) -> bool {
        !self.attempts.is_empty()
            && self.attempts.iter().all(|a| !a.status.is_pass() && !a.status.is_inconclusive())
    }

    /// Median measured wall-clock. `None` when nothing was measured — never 0.
    pub fn median_wall_ms(&self) -> Option<u64> {
        let mut v: Vec<u64> = self.attempts.iter().filter_map(|a| a.wall_ms).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(v[v.len() / 2])
    }
}

/// Run one task k times against an injected actuator, grading the real world after
/// each attempt.
///
/// The actuator is a closure, not a process: `inference/` never learns about argv
/// templates, environment policy, or process groups — that lives at the CLI edge
/// where the configuration is. The payoff is that every branch below is unit-
/// testable with a fake closure and **zero subprocesses**.
///
/// Each attempt gets a **fresh workspace**. That is the precondition for pass^k
/// meaning anything: if attempt 2 inherited attempt 1's world, the runs would not
/// be independent and the oracle would grade leftovers.
pub fn run_task<F>(spec: &McpSpec, task_id: &str, goal: &str, k: u32, mut actuate: F) -> TaskResult
where
    F: FnMut(&AttemptContext) -> ActuatorReport,
{
    let k = k.max(1);
    let mut attempts = Vec::with_capacity(k as usize);

    for n in 1..=k {
        // A harness failure is recorded, never propagated as a verdict: our own
        // machinery breaking must not read as the agent being wrong.
        let ws = match Workspace::seed(spec, task_id, goal) {
            Ok(ws) => ws,
            Err(e) => {
                attempts.push(Attempt {
                    n,
                    status: AttemptStatus::HarnessError { reason: crate::redact::redact_path(&e.to_string()) },
                    wall_ms: None,
                    exit_code: None,
                    stderr_tail: vec![],
                });
                continue;
            }
        };

        let ctx = AttemptContext {
            task_id,
            goal,
            workspace: ws.root(),
            task_file: ws.task_file(),
            otel_dir: ws.otel_dir(),
            db: ws.db(),
            attempt: n,
        };
        let report = actuate(&ctx);

        // Grade in every case where the agent actually ran — including a timeout,
        // so the report can say whether the work was done before it hung.
        let status = classify(&report, spec, &ws);
        let exit_code = match &report.result {
            // `None`, never 0 or -1: there was no exit code to observe.
            ActuatorResult::Exited { code } => Some(*code),
            ActuatorResult::TimedOut | ActuatorResult::SpawnFailed { .. } => None,
        };
        let wall_ms = match &report.result {
            ActuatorResult::SpawnFailed { .. } => None,
            _ => Some(report.wall_ms),
        };

        attempts.push(Attempt { n, status, wall_ms, exit_code, stderr_tail: report.stderr_tail.clone() });
    }

    TaskResult { task_id: task_id.to_string(), attempts, k }
}

/// Map one actuator outcome onto an attempt status.
///
/// Order matters: how the process ended is checked *before* the world is judged,
/// so a crash is never reported as "wrong answer".
fn classify(report: &ActuatorReport, spec: &McpSpec, ws: &Workspace) -> AttemptStatus {
    match &report.result {
        ActuatorResult::SpawnFailed { reason } => {
            AttemptStatus::AgentSpawnFailed { reason: crate::redact::redact_path(reason) }
        }
        ActuatorResult::TimedOut => {
            // Still grade: an agent that finished the work and then hung is a
            // different failure from one that hung without doing anything, and
            // the report must name which.
            let would_pass = grade(spec, ws).is_empty();
            AttemptStatus::AgentTimeout { limit_ms: report.wall_ms, oracle_would_have_passed: would_pass }
        }
        ActuatorResult::Exited { code } if *code != 0 => AttemptStatus::AgentExitNonZero { code: *code },
        ActuatorResult::Exited { .. } => {
            let failures = grade(spec, ws);
            if failures.is_empty() {
                AttemptStatus::Passed
            } else {
                AttemptStatus::FailedState { failures }
            }
        }
    }
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;

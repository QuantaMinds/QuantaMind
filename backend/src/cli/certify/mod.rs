//! `qm certify` — gate a deploy on the customer's own agent.
//!
//! Seeds a world, runs *their* command against it, grades the real end state, k
//! times. QuantaMind issues no model call and reads none of the agent's words:
//! their agent owns its model, and the grade is the world.

pub mod command;
pub mod render;
pub mod spawn;
pub mod suite;

use crate::cli::certify::command::AgentCommand;
use crate::cli::certify::suite::CertifyTask;
use crate::cli::run::render::FailOn;
use crate::inference::eval::harness::{is_discriminating, run_task, TaskResult};
use crate::inference::eval::readiness::types::Readiness;
use std::time::Duration;

pub struct CertifyOptions {
    pub command: AgentCommand,
    pub timeout: Duration,
    pub kill_grace: Duration,
    pub k_override: Option<u32>,
    pub fail_on: FailOn,
    pub quiet_agent: bool,
    pub no_precheck: bool,
}

/// Why a run stopped before measuring anything.
pub enum CertifyOutcome {
    Ran(CertifyReport),
    /// A task is vacuous — a do-nothing agent passes it. Aborts before any spawn,
    /// because a suite that can't fail would silently green-light every future run.
    NotDiscriminating { task_id: String },
    /// The suite itself is unusable (bad file, duplicate ids, k=0, missing deps).
    BadSuite(String),
}

pub struct CertifyReport {
    pub tasks: Vec<TaskResult>,
    /// The command TEMPLATE, pre-substitution. The expanded argv embeds an absolute
    /// path and must never reach a report (rule 7f).
    pub command_template: String,
    /// True when a suite has no `negative` task. Not an error — a small first suite
    /// is legitimate — but silent one-sidedness teaches over-triggering.
    pub one_sided: bool,
}

impl CertifyReport {
    /// **Ready** every task passed all k · **Conditional** flaky (no task failed
    /// every attempt, but not all are clean) · **NotReady** at least one task is
    /// broken outright.
    pub fn verdict(&self) -> Readiness {
        if self.tasks.iter().any(|t| t.failed_every_attempt()) {
            Readiness::NotReady
        } else if self.tasks.iter().all(|t| t.is_strict_pass()) {
            Readiness::Ready
        } else {
            Readiness::Conditional
        }
    }

    /// Every single attempt failed to even start the command. Distinguishes a
    /// misconfigured invocation (which retrying will never fix) from a transient
    /// harness fault.
    pub fn never_started(&self) -> bool {
        let mut any = false;
        for a in self.tasks.iter().flat_map(|t| &t.attempts) {
            any = true;
            if !matches!(a.status, crate::inference::eval::harness::AttemptStatus::AgentSpawnFailed { .. }) {
                return false;
            }
        }
        any
    }

    /// Any attempt we could not measure. Such a run can never exit 0: an
    /// incomplete measurement is not evidence of correctness.
    pub fn inconclusive(&self) -> bool {
        self.tasks.iter().any(|t| t.inconclusive())
    }
}

/// Run a whole suite.
///
/// Preflight order matters: everything that can reject the suite runs **before any
/// agent process is spawned**, so a broken suite costs zero agent invocations.
pub fn run_certify_suite(tasks: &[CertifyTask], opts: &CertifyOptions) -> CertifyOutcome {
    if tasks.is_empty() {
        return CertifyOutcome::BadSuite("the suite has no tasks".into());
    }

    if !opts.no_precheck {
        for t in tasks {
            match is_discriminating(&t.spec, &t.id, &t.goal) {
                Ok(true) => {}
                Ok(false) => return CertifyOutcome::NotDiscriminating { task_id: t.id.clone() },
                Err(e) => return CertifyOutcome::BadSuite(e),
            }
        }
    }

    let mut results = Vec::with_capacity(tasks.len());
    for t in tasks {
        let k = opts.k_override.unwrap_or(t.k);
        let r = run_task(&t.spec, &t.id, &t.goal, k, |ctx| {
            spawn::run_agent(&opts.command, ctx, opts.timeout, opts.kill_grace, !opts.quiet_agent)
        });
        results.push(r);
    }

    let one_sided = !tasks.iter().any(|t| t.is_negative);
    CertifyOutcome::Ran(CertifyReport {
        tasks: results,
        command_template: opts.command.template(),
        one_sided,
    })
}

//! Human output + the verdict→exit-code mapping for `qm certify`.

use crate::cli::certify::{CertifyOutcome, CertifyReport};
use crate::cli::run::render::{
    exit_code, FailOn, EXIT_INCONCLUSIVE, EXIT_NOTREADY, EXIT_READY, EXIT_UNREACHABLE,
};
use crate::inference::eval::harness::AttemptStatus;

/// Map a run to its process exit code.
///
/// Reuses the shared `exit_code` contract with one override: **an unmeasured
/// attempt reports 11 Inconclusive rather than a verdict.** This is an attribution
/// rule, not just a severity one. A missing `sqlite3` or an unstartable command is
/// *our* problem; letting it surface as 10/Conditional would tell the user their
/// agent is flaky when in fact we never measured it. 11 means "retry", which is
/// the honest instruction.
///
/// A **proven hard failure outranks it**: if some task failed every attempt, that
/// is a real, observed verdict and stays 20. Same precedent as
/// `cli/validate.rs::run_validate`, where static findings outrank an inconclusive
/// live check.
///
/// `--fail-on never` is honoured first — an explicit, printed operator decision to
/// treat the run as advisory.
pub fn certify_exit_code(report: &CertifyReport, fail_on: FailOn) -> i32 {
    if fail_on == FailOn::Never {
        return EXIT_READY;
    }
    // The command never started, not once. That is a configuration error — a
    // misspelled program, a missing interpreter — and retrying will not fix it, so
    // it must not report 11 ("retry"). 3 is the existing "system under test is not
    // runnable" code, the same meaning `doctor` gives an unreachable backend.
    if report.never_started() {
        return EXIT_UNREACHABLE;
    }
    let base = exit_code(report.verdict(), fail_on);
    if report.inconclusive() && base != EXIT_NOTREADY {
        return EXIT_INCONCLUSIVE;
    }
    base
}

/// Render the human report to stdout. `[QM-…]` notes go to stderr so `--json`
/// stays pipeable.
pub fn render(outcome: &CertifyOutcome, fail_on: FailOn) -> i32 {
    match outcome {
        CertifyOutcome::BadSuite(msg) => {
            eprintln!("[QM-BAD-SUITE] {msg}");
            2
        }
        CertifyOutcome::NotDiscriminating { task_id } => {
            eprintln!(
                "[QM-NOT-DISCRIMINATING] task '{task_id}' is vacuous — a do-nothing agent passes \
                 it, so it can never fail and would silently green-light every future run. No \
                 agent was started."
            );
            EXIT_NOTREADY
        }
        CertifyOutcome::Ran(r) => {
            println!("VERDICT: {:?}", r.verdict());
            println!();
            for t in &r.tasks {
                let mark = if t.is_strict_pass() { "PASS" } else { "FAIL" };
                let median = t
                    .median_wall_ms()
                    .map(|m| format!("{:.1}s", m as f64 / 1000.0))
                    // `None` means not measured. Never print 0.0s.
                    .unwrap_or_else(|| "n/a".into());
                println!("  {mark}  {}  {}/{}  median {median}", t.task_id, t.passes(), t.k);
                for a in t.attempts.iter().filter(|a| !a.status.is_pass()) {
                    println!("      attempt {}  {}", a.n, a.status.label());
                    for line in a.stderr_tail.iter().rev().take(3).rev() {
                        println!("          | {line}");
                    }
                }
            }

            // Totals name EVERY class, including the ones that aren't verdicts —
            // silent exclusion is the same bug as silent inclusion.
            let all = r.tasks.iter().flat_map(|t| &t.attempts);
            let (mut pass, mut state, mut exit, mut to, mut spawn, mut harness) = (0, 0, 0, 0, 0, 0);
            for a in all {
                match a.status {
                    AttemptStatus::Passed => pass += 1,
                    AttemptStatus::FailedState { .. } => state += 1,
                    AttemptStatus::AgentExitNonZero { .. } => exit += 1,
                    AttemptStatus::AgentTimeout { .. } => to += 1,
                    AttemptStatus::AgentSpawnFailed { .. } => spawn += 1,
                    AttemptStatus::HarnessError { .. } => harness += 1,
                }
            }
            println!();
            println!(
                "{} tasks · {} passed · {state} wrong-state · {exit} non-zero exit · {to} timeout \
                 · {spawn} could-not-start · {harness} harness error",
                r.tasks.len(),
                pass
            );
            println!("agent: {}   (template as configured)", r.command_template);
            println!("wall-clock only — QuantaMind does not observe your agent's model spend.");

            if r.one_sided {
                eprintln!(
                    "[QM-ONE-SIDED] every task in this suite rewards action. A one-sided suite \
                     teaches over-triggering; add a `negative` task where the correct answer is \
                     to refuse."
                );
            }
            if r.never_started() {
                eprintln!(
                    "[QM-AGENT-UNREACHABLE] the agent command never started on any attempt — \
                     check the program path. Retrying will not help."
                );
            } else if r.inconclusive() {
                eprintln!(
                    "[QM-INCONCLUSIVE] at least one attempt could not be measured — this run \
                     cannot be green regardless of the other results."
                );
            }
            let code = certify_exit_code(r, fail_on);
            if fail_on == FailOn::Never && r.verdict() != crate::inference::eval::readiness::types::Readiness::Ready {
                eprintln!("[QM-NOTE] verdict is {:?} but --fail-on never let it pass (exit 0)", r.verdict());
            }
            code
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

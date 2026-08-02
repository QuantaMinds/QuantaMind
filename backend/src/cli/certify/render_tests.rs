use super::*;
use crate::inference::eval::harness::{Attempt, TaskResult};
use crate::inference::eval::readiness::types::Readiness;

fn attempt(n: u32, status: AttemptStatus) -> Attempt {
    Attempt { n, status, wall_ms: Some(10), exit_code: None, stderr_tail: vec![] }
}
fn task(id: &str, statuses: Vec<AttemptStatus>) -> TaskResult {
    let k = statuses.len() as u32;
    TaskResult {
        task_id: id.into(),
        attempts: statuses.into_iter().enumerate().map(|(i, s)| attempt(i as u32 + 1, s)).collect(),
        k,
    }
}
fn report(tasks: Vec<TaskResult>) -> CertifyReport {
    CertifyReport { tasks, command_template: "agent {task}".into(), one_sided: false }
}
fn passed() -> AttemptStatus { AttemptStatus::Passed }
fn wrong() -> AttemptStatus { AttemptStatus::FailedState { failures: vec!["missing out.txt".into()] } }

#[test]
fn all_tasks_clean_is_ready_and_exits_zero() {
    let r = report(vec![task("a", vec![passed(), passed()])]);
    assert_eq!(r.verdict(), Readiness::Ready);
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_READY);
}

#[test]
fn a_flaky_task_is_conditional_and_exits_ten() {
    let r = report(vec![task("a", vec![passed(), wrong()])]);
    assert_eq!(r.verdict(), Readiness::Conditional);
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), 10);
}

#[test]
fn a_task_that_fails_every_attempt_is_not_ready_and_exits_twenty() {
    let r = report(vec![task("a", vec![wrong(), wrong()])]);
    assert_eq!(r.verdict(), Readiness::NotReady);
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_NOTREADY);
}

#[test]
fn fail_on_notready_tolerates_flake_but_not_breakage() {
    assert_eq!(certify_exit_code(&report(vec![task("a", vec![passed(), wrong()])]), FailOn::NotReady), EXIT_READY);
    assert_eq!(certify_exit_code(&report(vec![task("a", vec![wrong(), wrong()])]), FailOn::NotReady), EXIT_NOTREADY);
}

#[test]
fn fail_on_never_is_advisory_only() {
    assert_eq!(certify_exit_code(&report(vec![task("a", vec![wrong(), wrong()])]), FailOn::Never), EXIT_READY);
}

/// The rule that matters most for a CI gate: an unmeasured attempt can never
/// produce a green run, even when every measured task passed.
#[test]
fn an_otherwise_ready_run_with_a_harness_error_is_inconclusive_not_green() {
    let r = report(vec![
        task("a", vec![passed(), passed()]),
        task("b", vec![passed(), AttemptStatus::HarnessError { reason: "sqlite missing".into() }]),
    ]);
    assert!(r.inconclusive());
    assert_ne!(certify_exit_code(&r, FailOn::Conditional), EXIT_READY, "must not be green");
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_INCONCLUSIVE);
}

#[test]
fn a_spawn_failure_also_blocks_a_green_run() {
    let r = report(vec![task("a", vec![passed(), AttemptStatus::AgentSpawnFailed { reason: "no such file".into() }])]);
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_INCONCLUSIVE);
}

/// A crash is a measured verdict, so it takes the NotReady path — not 11.
#[test]
fn a_nonzero_exit_is_a_verdict_not_an_inconclusive() {
    let r = report(vec![task("a", vec![
        AttemptStatus::AgentExitNonZero { code: 3 },
        AttemptStatus::AgentExitNonZero { code: 3 },
    ])]);
    assert!(!r.inconclusive(), "a crash is observed, so it is a verdict");
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_NOTREADY);
}

#[test]
fn a_vacuous_suite_exits_twenty_before_any_agent_runs() {
    let code = render(&CertifyOutcome::NotDiscriminating { task_id: "t".into() }, FailOn::Conditional);
    assert_eq!(code, EXIT_NOTREADY);
}

#[test]
fn an_unusable_suite_exits_two() {
    assert_eq!(render(&CertifyOutcome::BadSuite("bad json".into()), FailOn::Conditional), 2);
}

/// A command that never started is a CONFIG error, not a transient one: retrying
/// will not create a missing binary. It must not report 11 ("retry").
#[test]
fn a_command_that_never_started_exits_three_not_eleven() {
    let fail = || AttemptStatus::AgentSpawnFailed { reason: "No such file".into() };
    let r = report(vec![task("a", vec![fail(), fail()])]);
    assert!(r.never_started());
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), 3);
}

/// But a command that started even once is not a config error — a mixed run keeps
/// the "we couldn't measure everything" reading.
#[test]
fn a_partially_started_run_is_inconclusive_not_unreachable() {
    let r = report(vec![task("a", vec![passed(), AttemptStatus::AgentSpawnFailed { reason: "x".into() }])]);
    assert!(!r.never_started());
    assert_eq!(certify_exit_code(&r, FailOn::Conditional), EXIT_INCONCLUSIVE);
}

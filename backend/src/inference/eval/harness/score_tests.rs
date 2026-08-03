use super::*;
use crate::inference::eval::mcp::oracle_fs::FsOracle;
use crate::inference::eval::mcp::world::FsSeed;
use std::collections::HashSet;

/// A world where the agent must create `out/done.txt`. Seeded with one file so the
/// world isn't empty; the oracle asserts something that does NOT exist yet, which
/// is what makes it discriminating.
fn spec() -> McpSpec {
    McpSpec::Fs {
        seed: FsSeed::from([("in.txt", "payload")]),
        oracle: FsOracle { assert_present: vec!["out/done.txt".into()], ..Default::default() },
    }
}

/// An actuator that does the work correctly.
fn good(ctx: &AttemptContext) -> ActuatorReport {
    std::fs::create_dir_all(ctx.workspace.join("out")).unwrap();
    std::fs::write(ctx.workspace.join("out/done.txt"), "ok").unwrap();
    ActuatorReport { result: ActuatorResult::Exited { code: 0 }, wall_ms: 10, stderr_tail: vec![] }
}

/// An actuator that exits cleanly having done nothing.
fn lazy(_: &AttemptContext) -> ActuatorReport {
    ActuatorReport { result: ActuatorResult::Exited { code: 0 }, wall_ms: 5, stderr_tail: vec![] }
}

#[test]
fn all_k_correct_is_a_strict_pass() {
    let r = run_task(&spec(), "t", "make out/done.txt", 3, good);
    assert_eq!(r.passes(), 3);
    assert!(r.is_strict_pass());
    assert!(!r.inconclusive());
}

#[test]
fn one_flaky_attempt_fails_strict_pass_k() {
    // 2 of 3 correct. pass@k would call this a pass; pass^k must not.
    let mut n = 0;
    let r = run_task(&spec(), "t", "g", 3, |ctx| {
        n += 1;
        if n == 2 { lazy(ctx) } else { good(ctx) }
    });
    assert_eq!(r.passes(), 2);
    assert!(!r.is_strict_pass(), "2/3 is not pass^3");
    assert!(!r.failed_every_attempt(), "flaky is not broken");
}

#[test]
fn zero_passes_is_broken_not_merely_flaky() {
    let r = run_task(&spec(), "t", "g", 3, lazy);
    assert_eq!(r.passes(), 0);
    assert!(r.failed_every_attempt());
}

#[test]
fn a_failing_attempt_carries_the_real_oracle_strings_not_a_summary() {
    let r = run_task(&spec(), "t", "g", 1, lazy);
    match &r.attempts[0].status {
        AttemptStatus::FailedState { failures } => {
            assert!(
                failures.iter().any(|f| f.contains("out/done.txt")),
                "the user must see WHICH assertion failed: {failures:?}"
            );
        }
        other => panic!("expected FailedState, got {other:?}"),
    }
}

/// A crash is a pass^k FAILURE, not "inconclusive". We are blind to the agent's
/// internals and cannot tell a provider 429 from an agent that doesn't retry —
/// calling it inconclusive would invent a cause we cannot observe.
#[test]
fn a_nonzero_exit_is_a_failure_even_when_the_world_is_correct() {
    let r = run_task(&spec(), "t", "g", 1, |ctx| {
        let mut rep = good(ctx); // world IS correct
        rep.result = ActuatorResult::Exited { code: 3 };
        rep
    });
    assert_eq!(r.attempts[0].status, AttemptStatus::AgentExitNonZero { code: 3 });
    assert!(!r.is_strict_pass());
    assert!(!r.inconclusive(), "a crash is observed, so it is a verdict — not inconclusive");
    assert_eq!(r.attempts[0].exit_code, Some(3));
}

/// An agent that finishes the work and then hangs is not deployable — so a timeout
/// fails even with a correct world. But the report must name the honest cause.
#[test]
fn a_timeout_fails_but_records_that_the_world_was_already_correct() {
    let r = run_task(&spec(), "t", "g", 1, |ctx| {
        let mut rep = good(ctx); // work done...
        rep.result = ActuatorResult::TimedOut; // ...then hung
        rep
    });
    match &r.attempts[0].status {
        AttemptStatus::AgentTimeout { oracle_would_have_passed, .. } => {
            assert!(*oracle_would_have_passed, "must record that the work WAS done");
            assert!(r.attempts[0].status.label().contains("hung after finishing"));
        }
        other => panic!("expected AgentTimeout, got {other:?}"),
    }
    assert!(!r.is_strict_pass(), "hanging is still a failure");
    assert_eq!(r.attempts[0].exit_code, None, "no exit code was observed — must not fabricate 0");
}

#[test]
fn a_timeout_with_an_incorrect_world_does_not_claim_the_work_was_done() {
    let r = run_task(&spec(), "t", "g", 1, |_| ActuatorReport {
        result: ActuatorResult::TimedOut,
        wall_ms: 1000,
        stderr_tail: vec![],
    });
    match &r.attempts[0].status {
        AttemptStatus::AgentTimeout { oracle_would_have_passed, .. } => {
            assert!(!*oracle_would_have_passed);
            assert!(!r.attempts[0].status.label().contains("hung after finishing"));
        }
        other => panic!("expected AgentTimeout, got {other:?}"),
    }
}

/// A spawn failure is neither a pass nor a verdict — but it must poison the run,
/// or an unmeasured suite could report green.
#[test]
fn a_spawn_failure_is_inconclusive_and_can_never_be_green() {
    let r = run_task(&spec(), "t", "g", 3, |ctx| {
        // Two good attempts and one that never started.
        if ctx.attempt == 2 {
            ActuatorReport {
                result: ActuatorResult::SpawnFailed { reason: "no such file".into() },
                wall_ms: 0,
                stderr_tail: vec![],
            }
        } else {
            good(ctx)
        }
    });
    assert!(r.inconclusive());
    assert!(!r.is_strict_pass(), "an incomplete run must never read as a pass");
    assert_eq!(r.attempts[1].exit_code, None);
    assert_eq!(r.attempts[1].wall_ms, None, "nothing ran — must not report 0ms");
}

/// pass^k independence: if attempts shared a workspace, attempt 2 would grade
/// attempt 1's leftovers and k runs would not be independent measurements.
#[test]
fn every_attempt_gets_its_own_workspace() {
    let mut seen: HashSet<std::path::PathBuf> = HashSet::new();
    let r = run_task(&spec(), "t", "g", 4, |ctx| {
        seen.insert(ctx.workspace.to_path_buf());
        good(ctx)
    });
    assert_eq!(seen.len(), 4, "each attempt must get a distinct workspace: {seen:?}");
    assert_eq!(r.passes(), 4);
}

/// The seeded world must actually be present for the agent — otherwise a task
/// that reads its input could never succeed.
#[test]
fn the_seed_is_present_in_every_attempts_workspace() {
    run_task(&spec(), "t", "g", 2, |ctx| {
        let seeded = std::fs::read_to_string(ctx.workspace.join("in.txt")).expect("seed must exist");
        assert_eq!(seeded, "payload");
        good(ctx)
    });
}

/// Nothing of ours may sit inside the graded root, or "end state" would include
/// harness artifacts rather than only the seed plus the agent's work.
#[test]
fn harness_files_live_outside_the_graded_root() {
    run_task(&spec(), "t", "g", 1, |ctx| {
        assert!(!ctx.task_file.starts_with(ctx.workspace), "task.json must not be graded");
        assert!(!ctx.otel_dir.starts_with(ctx.workspace), "otel/ must not be graded");
        assert!(ctx.task_file.exists(), "but the agent must still be able to read it");
        good(ctx)
    });
}

#[test]
fn attempt_numbers_are_one_based_and_sequential() {
    let mut got = vec![];
    run_task(&spec(), "t", "g", 3, |ctx| {
        got.push(ctx.attempt);
        good(ctx)
    });
    assert_eq!(got, vec![1, 2, 3]);
}

#[test]
fn k_zero_is_clamped_to_one_rather_than_silently_passing_zero_attempts() {
    // A task with k=0 that ran nothing must not report "all attempts passed".
    let r = run_task(&spec(), "t", "g", 0, good);
    assert_eq!(r.attempts.len(), 1);
    assert!(r.is_strict_pass());
}

#[test]
fn median_wall_is_none_when_nothing_was_measured() {
    let r = run_task(&spec(), "t", "g", 1, |_| ActuatorReport {
        result: ActuatorResult::SpawnFailed { reason: "x".into() },
        wall_ms: 0,
        stderr_tail: vec![],
    });
    assert_eq!(r.median_wall_ms(), None, "None means not measured — never 0");
}

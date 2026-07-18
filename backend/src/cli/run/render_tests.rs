use super::*;

#[test]
fn fail_on_conditional_is_the_default_gate() {
    assert_eq!(exit_code(Readiness::Ready, FailOn::Conditional), EXIT_READY);
    assert_eq!(exit_code(Readiness::Conditional, FailOn::Conditional), EXIT_CONDITIONAL);
    assert_eq!(exit_code(Readiness::NotReady, FailOn::Conditional), EXIT_NOTREADY);
}

#[test]
fn fail_on_notready_tolerates_conditional() {
    // A soft policy: Conditional must NOT fail the build, NotReady still must.
    assert_eq!(exit_code(Readiness::Ready, FailOn::NotReady), EXIT_READY);
    assert_eq!(exit_code(Readiness::Conditional, FailOn::NotReady), EXIT_READY);
    assert_eq!(exit_code(Readiness::NotReady, FailOn::NotReady), EXIT_NOTREADY);
}

#[test]
fn fail_on_never_is_advisory_only() {
    for s in [Readiness::Ready, Readiness::Conditional, Readiness::NotReady] {
        assert_eq!(exit_code(s, FailOn::Never), EXIT_READY);
    }
}

#[test]
fn measured_nothing_separates_an_errored_run_from_a_real_failure() {
    // No paths, or all-zero trials → couldn't measure → Inconclusive.
    assert!(measured_nothing(&[]));
    assert!(measured_nothing(&[0]));
    assert!(measured_nothing(&[0, 0]));
    // Any positive total_runs = the model actually ran (even if it lost every trial) →
    // a real NotReady, NOT inconclusive.
    assert!(!measured_nothing(&[5]));
    assert!(!measured_nothing(&[0, 5]));
}

#[test]
fn selection_accepts_only_in_range_1_based() {
    assert_eq!(parse_selection("1", 3), Some(0));
    assert_eq!(parse_selection(" 3 \n", 3), Some(2)); // trims whitespace/newline
    assert_eq!(parse_selection("0", 3), None); // 0 is out of the 1-based range
    assert_eq!(parse_selection("4", 3), None); // past the end
    assert_eq!(parse_selection("", 3), None);
    assert_eq!(parse_selection("two", 3), None);
}

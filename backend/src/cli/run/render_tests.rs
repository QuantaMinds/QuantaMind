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
fn selection_accepts_only_in_range_1_based() {
    assert_eq!(parse_selection("1", 3), Some(0));
    assert_eq!(parse_selection(" 3 \n", 3), Some(2)); // trims whitespace/newline
    assert_eq!(parse_selection("0", 3), None); // 0 is out of the 1-based range
    assert_eq!(parse_selection("4", 3), None); // past the end
    assert_eq!(parse_selection("", 3), None);
    assert_eq!(parse_selection("two", 3), None);
}

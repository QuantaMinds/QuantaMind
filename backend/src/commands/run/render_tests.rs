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

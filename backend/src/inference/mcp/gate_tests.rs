use super::*;
use serde_json::json;

fn call(read_only: bool, trusted: bool) -> PendingCall {
    PendingCall {
        server_id: "fs".into(),
        tool: "write_file".into(),
        read_only,
        trusted_server: trusted,
        args: json!({}),
    }
}

#[test]
fn sandbox_auto_approves_everything_even_writes() {
    // Safe by isolation — a disposable sandbox approves even an untrusted write.
    let write = call(false, false);
    assert_eq!(gate_decision(GatePolicy::SandboxAutoApprove, &write, None), Decision::Approve);
}

#[test]
fn real_tools_auto_approve_only_trusted_read_only() {
    let p = GatePolicy::RealToolsReadOnlyAuto;
    // read-only + trusted → auto approve, no human needed.
    assert_eq!(gate_decision(p, &call(true, true), None), Decision::Approve);
    // read-only but UNTRUSTED server (poisoned readOnlyHint) → deny without approval.
    assert_eq!(gate_decision(p, &call(true, false), None), Decision::Deny);
    // a WRITE is never auto-approved, even on a trusted server.
    assert_eq!(gate_decision(p, &call(false, true), None), Decision::Deny);
    // ...but an explicit human Approve lets the write through.
    assert_eq!(gate_decision(p, &call(false, true), Some(Decision::Approve)), Decision::Approve);
    // an explicit Deny is honored.
    assert_eq!(gate_decision(p, &call(true, true), Some(Decision::Deny)), Decision::Approve, "trusted read-only is auto-approved before consulting external");
}

#[test]
fn deny_by_default_needs_an_explicit_approve() {
    let p = GatePolicy::DenyByDefault;
    assert_eq!(gate_decision(p, &call(true, true), None), Decision::Deny, "no decision → deny");
    assert_eq!(gate_decision(p, &call(false, false), Some(Decision::Approve)), Decision::Approve);
    assert_eq!(gate_decision(p, &call(true, true), Some(Decision::Deny)), Decision::Deny);
}

#[test]
fn absent_decision_is_always_deny_for_real_tools() {
    // The load-bearing invariant: timeout / closed UI / unknown → deny.
    for c in [call(false, false), call(false, true), call(true, false)] {
        assert_eq!(gate_decision(GatePolicy::RealToolsReadOnlyAuto, &c, None), Decision::Deny);
        assert_eq!(gate_decision(GatePolicy::DenyByDefault, &c, None), Decision::Deny);
    }
}

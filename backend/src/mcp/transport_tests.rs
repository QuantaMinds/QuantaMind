//! Pure framing/correlation tests — no subprocess. Drives `route_line`
//! directly, which is where the load-bearing logic lives (id-correlation,
//! notification-ignore, stdout-garbage capture).

use super::{route_line, Garbage, Pending};
use crate::mcp::wire::RequestId;
use crate::sync::MutexExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

fn fresh() -> (Pending, Garbage) {
    (Arc::new(Mutex::new(HashMap::new())), Arc::new(Mutex::new(None)))
}

#[test]
fn response_is_routed_to_its_waiter_by_id() {
    let (pending, garbage) = fresh();
    let (tx, mut rx) = oneshot::channel();
    pending.lock_recover().insert(RequestId::Num(2), tx);

    route_line(r#"{"jsonrpc":"2.0","id":2,"result":{"ok":true}}"#, &pending, &garbage);

    let resp = rx.try_recv().expect("waiter received its response");
    assert!(resp.result().is_ok());
    assert!(pending.lock_recover().is_empty(), "the waiter was consumed");
    assert!(garbage.lock_recover().is_none());
}

#[test]
fn error_response_routes_on_the_error_arm() {
    let (pending, garbage) = fresh();
    let (tx, mut rx) = oneshot::channel();
    pending.lock_recover().insert(RequestId::Num(3), tx);

    route_line(
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"Method not found"}}"#,
        &pending,
        &garbage,
    );

    let resp = rx.try_recv().unwrap();
    assert_eq!(resp.result().unwrap_err().code, -32601);
}

#[test]
fn notification_is_ignored_not_treated_as_garbage() {
    let (pending, garbage) = fresh();
    route_line(r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#, &pending, &garbage);
    assert!(garbage.lock_recover().is_none(), "valid JSON is not garbage");
    assert!(pending.lock_recover().is_empty());
}

#[test]
fn non_json_stdout_is_recorded_as_garbage() {
    let (pending, garbage) = fresh();
    route_line("INFO server listening — logging to stdout by mistake", &pending, &garbage);
    let g = garbage.lock_recover().clone().expect("garbage recorded");
    assert!(g.contains("server listening"));
}

#[test]
fn only_the_first_garbage_line_is_kept() {
    let (pending, garbage) = fresh();
    route_line("first junk", &pending, &garbage);
    route_line("second junk", &pending, &garbage);
    assert_eq!(garbage.lock_recover().clone().unwrap(), "first junk");
}

#[test]
fn a_response_with_an_unknown_id_is_dropped_without_panic() {
    let (pending, garbage) = fresh();
    route_line(r#"{"jsonrpc":"2.0","id":99,"result":{}}"#, &pending, &garbage);
    assert!(garbage.lock_recover().is_none());
}

/// True if any process is still in the given process group.
#[cfg(unix)]
fn group_has_members(pgid: u32) -> bool {
    std::process::Command::new("pgrep")
        .arg("-g")
        .arg(pgid.to_string())
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

/// The Phase 3 orphan proof: a server that forks a distinct grandchild must be
/// killed as a WHOLE GROUP, not just the direct child. `sh -c 'sleep 300 & wait'`
/// forces `sleep` into a separate pid inside the child's group — the exact shape
/// `npx`→`node` creates. After `kill()`, nothing may remain in the group.
#[cfg(unix)]
#[test]
fn kill_terminates_the_whole_process_group_leaving_no_orphan() {
    let t = super::McpTransport::spawn("sh", &["-c".into(), "sleep 300 & wait".into()])
        .expect("spawn sh");
    let pgid = t.pid(); // process_group(0) → the child leads its own group
    std::thread::sleep(std::time::Duration::from_millis(300)); // let `sleep` spawn

    assert!(group_has_members(pgid), "group should be populated before kill");
    t.kill();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!group_has_members(pgid), "no process may survive in the group after kill");
}

// ── group-signal argv (the Linux `kill -TERM -<pgid>` self-kill) ──────────────────────

/// THE REGRESSION. `kill -TERM -<pgid>` without `--` makes procps `kill` (Linux) read the
/// leading-dash pgid as bundled short options and signal the CALLER'S OWN process group —
/// so `McpTransport::kill()` killed QuantaMind and ORPHANED the server it meant to reap,
/// the exact inverse of its purpose. Measured on ubuntu-22.04: `/bin/kill -TERM -4192`
/// (child correctly leading group 4192, caller in 4191) killed the caller with SIGTERM
/// (exit 143) and left the group alive; `/bin/kill -TERM -- -4192` exited 0, killed the
/// group, and the caller lived. macOS's BSD kill parses the bare form fine, which is why
/// this only ever bit Linux — and why CI's ubuntu job died mid-suite rather than failing.
#[test]
fn a_group_signal_separates_options_so_the_pgid_is_never_read_as_flags() {
    let argv = super::group_signal_argv("-TERM", 4192);
    assert_eq!(argv, vec!["-TERM", "--", "-4192"], "the `--` separator is load-bearing");
    // Stated as the invariant: the negative pid must come AFTER an end-of-options marker,
    // or `kill` is free to read it as flags and signal us instead.
    let sep = argv.iter().position(|a| a == "--").expect("an end-of-options separator is required");
    let target = argv.iter().position(|a| a == "-4192").expect("the pgid is the target");
    assert!(sep < target, "`--` must precede the negative pid: {argv:?}");
}

/// `kill -- -0` is `kill(0, sig)` — POSIX for "signal MY OWN process group". A real child
/// always has a non-zero pid, so a 0 here is only ever a bug; it must signal nothing rather
/// than kill the app. Guarded because this is the same class of mistake as the one above.
#[cfg(unix)]
#[test]
fn signalling_group_zero_is_refused_because_it_would_target_our_own_group() {
    // Must not panic, must not signal: we are still alive to make this assertion.
    super::signal_group("-TERM", 0);
    super::signal_group("-KILL", 0);
}

/// The live proof, and the one that actually caught this: reaping a child's group must not
/// take the CALLER down with it. Runs the real `kill()` path against a real child and then
/// asserts the test process is still running — on Linux this whole binary used to die here.
#[cfg(unix)]
#[test]
fn killing_a_childs_group_leaves_the_caller_alive() {
    let t = super::McpTransport::spawn("sh", &["-c".into(), "sleep 300 & wait".into()])
        .expect("spawn sh");
    std::thread::sleep(std::time::Duration::from_millis(300));
    t.kill();
    // Reaching this line at all is the assertion: pre-fix, `kill()` SIGTERM'd our own
    // process group and the harness never got here.
    assert!(std::process::id() > 0, "the caller survived reaping the child's group");
}

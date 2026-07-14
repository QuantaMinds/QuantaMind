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

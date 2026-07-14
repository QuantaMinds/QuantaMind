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

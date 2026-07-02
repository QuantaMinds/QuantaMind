use super::has_bin;
use crate::commands::llama::llama_runtime::bin_name;
use crate::commands::llama::llama_server_types::{
    LlamaProbeReadiness, LlamaServerState, LlamaStartResult, SpawnReadout,
};

#[test]
fn has_bin_requires_the_binary_in_the_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(has_bin(dir.path().to_path_buf()).is_none(), "empty dir resolves to None");
    std::fs::write(dir.path().join(bin_name()), b"x").expect("write");
    assert_eq!(
        has_bin(dir.path().to_path_buf()).as_deref(),
        Some(dir.path()),
        "dir containing llama-server resolves to itself",
    );
}

#[test]
fn already_running_serializes_with_status_tag() {
    let json = serde_json::to_string(&LlamaStartResult::AlreadyRunning).unwrap();
    assert_eq!(json, r#"{"status":"already_running"}"#);
}

#[test]
fn started_serializes_with_pid_and_port() {
    // No constraint applied → `note` omitted (serde skips `None`), so the roomy-machine
    // payload is unchanged from before the hardware-plan field was added.
    let json = serde_json::to_string(&LlamaStartResult::Started { pid: 42, port: 8081, note: None }).unwrap();
    assert_eq!(json, r#"{"status":"started","pid":42,"port":8081}"#);
}

#[test]
fn started_includes_note_when_a_hardware_constraint_was_applied() {
    let json =
        serde_json::to_string(&LlamaStartResult::Started { pid: 42, port: 8081, note: Some("Q8 KV".into()) }).unwrap();
    assert!(json.contains(r#""note":"Q8 KV""#), "the constraint note reaches the UI: {json}");
}

#[test]
fn not_bundled_serializes_with_note() {
    let r = LlamaStartResult::NotBundled { note: "no binary".into() };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""status":"not_bundled""#));
    assert!(json.contains(r#""note":"no binary""#));
}

#[test]
fn start_failed_serializes_with_error() {
    let r = LlamaStartResult::StartFailed { error: "boom".into() };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""status":"start_failed""#));
    assert!(json.contains(r#""error":"boom""#));
}

#[test]
fn is_current_is_false_when_nothing_is_running() {
    // No server up → never "current", so a start always proceeds rather than
    // short-circuiting to AlreadyRunning.
    let state = LlamaServerState::default();
    assert!(!state.is_current("/g/phi3.gguf", 8192));
}

#[test]
fn probe_readiness_reports_not_running_when_idle() {
    // Nothing up → the probe must tell the user to start the model, not run blind.
    let state = LlamaServerState::default();
    assert_eq!(state.probe_readiness("/g/phi3.gguf"), LlamaProbeReadiness::NotRunning);
}

/// The flagged edge: identity is matched on the EXACT launch path, so a probe of a
/// model the server didn't load reports `WrongModel` — never a false `Ready` that
/// would score the loaded model's weights instead.
#[cfg(unix)]
#[test]
fn probe_readiness_matches_on_exact_model_path() {
    let child = std::process::Command::new("sleep").arg("30").spawn().expect("spawn dummy child");
    let state = LlamaServerState::default();
    state.store(child, "/g/phi3.gguf".into(), 8192);
    assert_eq!(state.probe_readiness("/g/phi3.gguf"), LlamaProbeReadiness::Ready { ctx: 8192 });
    assert_eq!(state.probe_readiness("/g/llama3.gguf"), LlamaProbeReadiness::WrongModel);
    state.stop().expect("stop dummy child");
}

#[test]
fn readout_is_none_until_a_server_is_ready() {
    // No server up (or a start that never reached the ready arm) → no fabricated
    // readout. set_readout is a no-op with nothing running.
    let state = LlamaServerState::default();
    assert_eq!(state.readout(), None);
    state.set_readout(SpawnReadout { model_bytes: Some(1), load_ms: 5 });
    assert_eq!(state.readout(), None, "set_readout no-ops without a running server");
}

#[test]
fn spawn_readout_serializes_with_model_bytes_and_load_ms() {
    let json = serde_json::to_string(&SpawnReadout { model_bytes: Some(4_600_000_000), load_ms: 7000 }).unwrap();
    assert!(json.contains(r#""model_bytes":4600000000"#));
    assert!(json.contains(r#""load_ms":7000"#));
    // Unknown footprint serializes as null (never a fake 0).
    let unknown = serde_json::to_string(&SpawnReadout { model_bytes: None, load_ms: 100 }).unwrap();
    assert!(unknown.contains(r#""model_bytes":null"#));
}

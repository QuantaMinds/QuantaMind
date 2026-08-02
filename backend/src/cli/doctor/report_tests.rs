use super::*;
use crate::commands::remote::remote_health::{RemoteAuthReport, RemoteAuthStatus};

fn local(kind: BackendKind, models: &[&str]) -> BackendDoctor {
    BackendDoctor {
        kind,
        endpoint: "http://localhost:1".into(),
        reachable: true,
        version: None,
        models: models.iter().map(|s| s.to_string()).collect(),
        credential: None,
        native_fc: NativeFc::NotProbed,
    }
}

fn remote(kind: BackendKind, status: RemoteAuthStatus, models: &[&str]) -> BackendDoctor {
    let reachable = !matches!(status, RemoteAuthStatus::Unreachable | RemoteAuthStatus::TlsError | RemoteAuthStatus::Unconfigured);
    BackendDoctor {
        kind,
        endpoint: "http://gpu:8000".into(),
        reachable,
        version: None,
        models: models.iter().map(|s| s.to_string()).collect(),
        credential: Some(RemoteAuthReport { status, http_status: None, host: "http://gpu:8000".into(), insecure_key: false }),
        native_fc: NativeFc::NotProbed,
    }
}

#[test]
fn local_reachable_with_a_model_is_runnable() {
    assert!(local(BackendKind::LlamaCpp, &["qwen2.5:3b"]).runnable());
}

#[test]
fn reachable_but_zero_models_is_not_runnable() {
    // The first-run trap: up, green-looking, but nothing to run.
    let b = local(BackendKind::LlamaCpp, &[]);
    assert!(b.reachable);
    assert!(!b.runnable());
}

#[test]
fn remote_needs_both_a_model_and_an_ok_credential() {
    // Ok + models → runnable.
    assert!(remote(BackendKind::VLlm, RemoteAuthStatus::Ok, &["m"]).runnable());
    // Ok but empty → not runnable.
    assert!(!remote(BackendKind::VLlm, RemoteAuthStatus::Ok, &[]).runnable());
    // A rejected key is never runnable, even if a model list somehow came back.
    assert!(!remote(BackendKind::VLlm, RemoteAuthStatus::Unauthorized, &["m"]).runnable());
}

#[test]
fn exit_code_is_zero_iff_something_is_runnable() {
    let ready = DoctorReport { backends: vec![local(BackendKind::LlamaCpp, &["m"]), local(BackendKind::VLlm, &[])] };
    assert_eq!(ready.exit_code(), EXIT_OK);

    // Nothing runnable — including scan-mode with everything down — must be 3, not 0.
    let empty = DoctorReport { backends: vec![local(BackendKind::LlamaCpp, &[]), remote(BackendKind::VLlm, RemoteAuthStatus::Unreachable, &[])] };
    assert_eq!(empty.exit_code(), EXIT_NO_RUNNABLE);
}

#[test]
fn report_round_trips_through_json() {
    let r = DoctorReport { backends: vec![local(BackendKind::LlamaCpp, &["qwen2.5:3b"]), remote(BackendKind::VLlm, RemoteAuthStatus::Unauthorized, &[])] };
    let json = serde_json::to_string(&r).unwrap();
    let back: DoctorReport = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

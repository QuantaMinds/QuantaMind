use super::*;
use crate::commands::remote::remote_health::RemoteAuthReport;
use crate::commands::doctor::report::DoctorReport;

fn remote(status: RemoteAuthStatus, insecure_key: bool) -> BackendDoctor {
    BackendDoctor {
        kind: BackendKind::VLlm,
        endpoint: "http://gpu:8000".into(),
        reachable: !matches!(status, RemoteAuthStatus::Unreachable | RemoteAuthStatus::Unconfigured | RemoteAuthStatus::TlsError),
        version: None,
        models: vec![],
        credential: Some(RemoteAuthReport { status, http_status: None, host: "http://gpu:8000".into(), insecure_key }),
        native_fc: NativeFc::NotProbed,
    }
}

fn ollama(reachable: bool, models: &[&str]) -> BackendDoctor {
    BackendDoctor {
        kind: BackendKind::Ollama,
        endpoint: "http://localhost:11434".into(),
        reachable,
        version: reachable.then(|| "0.24.0".to_string()),
        models: models.iter().map(|s| s.to_string()).collect(),
        credential: None,
        native_fc: NativeFc::NotProbed,
    }
}

fn lines_for(b: BackendDoctor) -> Vec<String> {
    error_lines(&DoctorReport { backends: vec![b] })
}

/// The three failure modes the whole doctor exists to keep distinct MUST NOT swap:
/// a down server, a rejected key, and a key withheld over http are opposite fixes.
#[test]
fn a_down_server_says_unreachable_not_bad_key() {
    let l = lines_for(remote(RemoteAuthStatus::Unreachable, false));
    assert!(l.iter().any(|s| s.contains("[QM-BACKEND-UNREACHABLE]")), "{l:?}");
    assert!(!l.iter().any(|s| s.contains("QM-UNAUTHORIZED")), "a down server must never read as a bad key: {l:?}");
}

#[test]
fn a_rejected_key_says_unauthorized_not_unreachable() {
    let l = lines_for(remote(RemoteAuthStatus::Unauthorized, false));
    assert!(l.iter().any(|s| s.contains("[QM-UNAUTHORIZED]") && s.contains("check QM_API_KEY")), "{l:?}");
    assert!(!l.iter().any(|s| s.contains("UNREACHABLE")), "a bad key must never read as unreachable: {l:?}");
}

#[test]
fn a_key_over_plain_http_says_withheld() {
    // insecure_key is surfaced even independent of the blocking status.
    let l = lines_for(remote(RemoteAuthStatus::Unreachable, true));
    assert!(l.iter().any(|s| s.contains("[QM-INSECURE-KEY]") && s.contains("withheld")), "{l:?}");
}

#[test]
fn ollama_up_with_zero_models_is_a_loud_no_models_finding() {
    // Reachable but empty → the most common first-run trap. NOT a green line.
    let l = lines_for(ollama(true, &[]));
    assert!(l.iter().any(|s| s.contains("[QM-NO-MODELS]") && s.contains("ollama pull")), "{l:?}");
}

#[test]
fn a_healthy_scan_emits_no_blocker_spam() {
    // Ollama runnable + vLLM simply off → no fix line for the backend that's just not running.
    let report = DoctorReport { backends: vec![ollama(true, &["qwen2.5:3b"]), remote(RemoteAuthStatus::Unreachable, false)] };
    assert!(error_lines(&report).is_empty(), "a healthy scan should be quiet: {:?}", error_lines(&report));
    assert_eq!(report.exit_code(), crate::commands::doctor::report::EXIT_OK);
}

#[test]
fn human_report_names_the_next_command_when_ready() {
    let report = DoctorReport { backends: vec![ollama(true, &["qwen2.5:3b"])] };
    let text = render_human(&report);
    assert!(text.contains("qm run --backend ollama"), "{text}");
    assert!(text.contains("✓ ready"), "{text}");
}

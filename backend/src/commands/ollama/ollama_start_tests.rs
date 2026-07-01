use super::*;

#[test]
fn already_running_serializes_with_status_tag() {
    let json = serde_json::to_string(&OllamaStartResult::AlreadyRunning).unwrap();
    assert_eq!(json, r#"{"status":"already_running"}"#);
}

#[test]
fn started_serializes_with_pid() {
    let json = serde_json::to_string(&OllamaStartResult::Started { pid: 1234 }).unwrap();
    assert_eq!(json, r#"{"status":"started","pid":1234}"#);
}

#[test]
fn not_installed_serializes_with_install_url() {
    let r = OllamaStartResult::NotInstalled { install_url: INSTALL_URL.into() };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""status":"not_installed""#));
    assert!(json.contains(r#""install_url":"https://ollama.com/download""#));
}

#[test]
fn start_failed_serializes_with_error() {
    let r = OllamaStartResult::StartFailed { error: "port in use".into() };
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(json, r#"{"status":"start_failed","error":"port in use"}"#);
}

#[test]
fn manual_start_required_serializes_with_install_url() {
    let r = OllamaStartResult::ManualStartRequired { install_url: INSTALL_URL.into() };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""status":"manual_start_required""#));
    assert!(json.contains(r#""install_url":"https://ollama.com/download""#));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn auto_start_is_unsupported_off_macos() {
    assert!(!AUTO_START_SUPPORTED);
}

#[cfg(target_os = "macos")]
#[test]
fn auto_start_is_supported_on_macos() {
    assert!(AUTO_START_SUPPORTED);
}

#[test]
fn stop_owned_is_a_noop_when_we_started_nothing() {
    // AlreadyRunning (a user's daemon) leaves started_pid None → reap touches nothing.
    let s = OllamaStartState::default();
    assert!(s.stop_owned().is_ok());
    assert!(s.stop_owned().is_ok(), "idempotent");
}

#[cfg(target_os = "macos")]
#[test]
fn stop_owned_kills_the_app_spawned_pid_only() {
    use std::process::{Command, Stdio};
    use std::time::Duration;
    let mut child = Command::new("sleep").arg("30").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let s = OllamaStartState::default();
    s.remember(child.id());
    s.stop_owned().unwrap();
    let mut dead = false;
    for _ in 0..40 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            dead = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(dead, "the app-spawned pid was reaped");
    assert!(s.stop_owned().is_ok(), "idempotent after the pid is taken");
}

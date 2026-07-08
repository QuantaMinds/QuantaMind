use super::*;

fn with_vllm(url: &str, key: Option<&str>) -> UserSettings {
    UserSettings {
        vllm_url: Some(url.into()),
        vllm_api_key: key.map(Into::into),
        ..UserSettings::default()
    }
}

#[test]
fn rejects_api_key_on_cleartext_remote_url() {
    let s = with_vllm("http://34.10.20.30:8000", Some("sk-secret"));
    let err = reject_cleartext_credentials(&s).unwrap_err();
    assert!(matches!(err, AppError::Validation(_)));
    // Message must name the fix, not just the failure.
    assert!(err.to_string().contains("HTTPS"));
}

#[test]
fn allows_api_key_over_https() {
    let s = with_vllm("https://34.10.20.30:8000", Some("sk-secret"));
    assert!(reject_cleartext_credentials(&s).is_ok());
}

#[test]
fn allows_api_key_over_loopback_http() {
    // A local server on http is fine — there is no network path to sniff.
    let s = with_vllm("http://127.0.0.1:8000", Some("sk-secret"));
    assert!(reject_cleartext_credentials(&s).is_ok());
}

#[test]
fn allows_cleartext_remote_url_when_no_key() {
    // No credential to leak → the transport doesn't matter.
    let s = with_vllm("http://34.10.20.30:8000", None);
    assert!(reject_cleartext_credentials(&s).is_ok());
}

#[test]
fn checks_sglang_too() {
    let s = UserSettings {
        sglang_url: Some("http://example.com:30000".into()),
        sglang_api_key: Some("sk-secret".into()),
        ..UserSettings::default()
    };
    assert!(reject_cleartext_credentials(&s).is_err());
}

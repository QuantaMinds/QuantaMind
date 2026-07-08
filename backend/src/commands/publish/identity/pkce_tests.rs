use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[test]
fn challenge_is_stable_and_url_safe_no_pad() {
    let c = pkce_challenge("verifier-123");
    assert_eq!(c, pkce_challenge("verifier-123")); // deterministic
    assert!(!c.contains('=') && !c.contains('+') && !c.contains('/')); // url-safe, no pad
    assert_ne!(c, pkce_challenge("verifier-124"));
}

#[test]
fn pkce_pair_verifier_is_in_range_and_challenge_matches() {
    let (v, c) = pkce_pair();
    assert!((43..=128).contains(&v.len()));
    assert_eq!(c, pkce_challenge(&v));
}

#[test]
fn parses_params_from_request_line() {
    let raw = "GET /callback?code=abc123&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
    assert_eq!(parse_code_from_request(raw).as_deref(), Some("abc123"));
    assert_eq!(parse_param_from_request(raw, "state").as_deref(), Some("xyz"));
    assert_eq!(parse_code_from_request("GET /callback?state=only HTTP/1.1"), None);
}

/// Helper: run one callback request through await_redirect and return the result.
async fn drive_callback(request: &[u8], expected_state: &str) -> AppResult<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = expected_state.to_string();
    let server = tokio::spawn(async move { await_redirect(listener, &state).await });
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(request).await.unwrap();
    let mut resp = Vec::new();
    let _ = client.read_to_end(&mut resp).await;
    server.await.unwrap()
}

#[tokio::test]
async fn await_redirect_returns_code_when_host_and_state_match() {
    let req = b"GET /callback?code=loopback_ok&state=s HTTP/1.1\r\nHost: 127.0.0.1:5\r\n\r\n";
    assert_eq!(drive_callback(req, "s").await.unwrap(), "loopback_ok");
}

#[tokio::test]
async fn await_redirect_rejects_state_mismatch() {
    // A callback with the wrong state (CSRF/forged) is refused even with a loopback Host.
    let req = b"GET /callback?code=x&state=WRONG HTTP/1.1\r\nHost: 127.0.0.1:5\r\n\r\n";
    assert!(drive_callback(req, "expected").await.is_err());
}

#[tokio::test]
async fn await_redirect_rejects_non_loopback_host() {
    // A DNS-rebinding page reaching the port carries its own Host — rejected.
    let req = b"GET /callback?code=x&state=s HTTP/1.1\r\nHost: evil.example.com\r\n\r\n";
    assert!(drive_callback(req, "s").await.is_err());
}

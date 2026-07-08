use crate::errors::{AppError, AppResult};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

/// The S256 code challenge for a verifier: base64url(no-pad) of SHA-256(verifier).
/// Pure + deterministic so the client and the auth server agree byte-for-byte.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh (verifier, challenge) pair. The verifier is two v4 UUIDs in hex (64
/// chars) — high-entropy and within PKCE's 43–128-char range; no new RNG dep.
pub fn pkce_pair() -> (String, String) {
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = pkce_challenge(&verifier);
    (verifier, challenge)
}

/// A fresh, high-entropy OAuth `state` nonce (CSRF binding): sent on the authorize URL and
/// required to match on the callback, so a forged/injected callback is rejected.
pub fn new_state() -> String {
    Uuid::new_v4().simple().to_string()
}

/// Extract query param `key` from the raw redirect request line
/// ("GET /callback?code=…&state=… HTTP/1.1"). Returns `None` if absent.
pub fn parse_param_from_request(raw: &str, key: &str) -> Option<String> {
    let path = raw.split_whitespace().nth(1)?; // the request-target
    let query = path.split_once('?')?.1;
    let prefix = format!("{key}=");
    query.split('&').find_map(|kv| kv.strip_prefix(&prefix).map(|c| c.to_string()))
}

/// Back-compat shim: the OAuth `code` param.
pub fn parse_code_from_request(raw: &str) -> Option<String> {
    parse_param_from_request(raw, "code")
}

/// The callback request's `Host` header must be a loopback literal. A DNS-rebinding page that
/// reached our ephemeral callback port would carry ITS hostname here, not `127.0.0.1` — so a
/// non-loopback (or missing) Host is rejected. A real browser redirect always sends loopback.
fn host_is_loopback(raw: &str) -> bool {
    let Some(host_line) = raw
        .lines()
        .find_map(|l| l.strip_prefix("Host:").or_else(|| l.strip_prefix("host:")))
    else {
        return false;
    };
    let host = host_line.trim();
    // Strip a trailing :port (only when the suffix is all digits, so IPv6 literals survive).
    let host = match host.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

const REDIRECT_PAGE: &str = "<html><body>You can close this tab and return to QuantaMind.</body></html>";

/// Block on the single OAuth loopback redirect, return its `code`, and reply with a
/// "you can close this tab" page. The listener binds an ephemeral 127.0.0.1 port
/// (caller reads `local_addr()` to build the redirect_uri). `expected_state` is the nonce
/// sent on the authorize URL: the callback is rejected unless its Host is loopback (DNS-
/// rebinding guard) AND its `state` matches (CSRF guard) — PKCE then protects the exchange.
pub async fn await_redirect(listener: TcpListener, expected_state: &str) -> AppResult<String> {
    let (mut stream, _) = listener.accept().await.map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| AppError::Io(e.to_string()))?;
    let raw = String::from_utf8_lossy(&buf[..n]);
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        REDIRECT_PAGE.len(),
        REDIRECT_PAGE
    );
    let _ = stream.write_all(resp.as_bytes()).await;

    if !host_is_loopback(&raw) {
        return Err(AppError::Validation("OAuth callback rejected: non-loopback Host".into()));
    }
    match parse_param_from_request(&raw, "state") {
        Some(s) if s == expected_state => {}
        _ => return Err(AppError::Validation("OAuth callback rejected: state mismatch".into())),
    }
    parse_param_from_request(&raw, "code")
        .ok_or_else(|| AppError::Validation("OAuth redirect carried no code".into()))
}

#[cfg(test)]
#[path = "pkce_tests.rs"]
mod tests;

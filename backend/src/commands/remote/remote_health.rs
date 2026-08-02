use crate::commands::system::health::HealthStatus;
#[cfg(feature = "gui")]
use crate::inference::backend::remote_config;
use crate::inference::backend::remote_guard::credential_allowed;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// The outcome of resolving a remote endpoint's credential — the FULL failure space, so a
/// bad key and a down server are never conflated (their fixes are opposite). Emitted to the
/// pre-flight so the run fails fast with the right, actionable message.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAuthStatus {
    /// 2xx AND the body is an OpenAI-style models list.
    Ok,
    /// Empty URL — no network call made.
    Unconfigured,
    /// Connection refused or timed out — the server, not the key. Fix: start it / check URL.
    Unreachable,
    /// TLS/certificate handshake failure — a transport problem, not the key.
    TlsError,
    /// 401 / 403 — the server is up and rejected the credential. Fix: check the API key.
    Unauthorized,
    /// 404 — reachable but no `/v1/models` (wrong path / not an OpenAI-compatible server).
    NotFound,
    /// Any other non-2xx (e.g. 5xx), or a 2xx whose body isn't a models list.
    ServerError,
}

/// A classified credential-resolution result. Carries ONLY a redacted `host` — never the
/// api_key, and never a URL with embedded `user:pass@` (rule 7f: no credential leaks into a
/// report/UI/log). `insecure_key` = a key is configured but the URL isn't https/loopback, so
/// the key was WITHHELD (we never transmit a credential over plain http).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RemoteAuthReport {
    pub status: RemoteAuthStatus,
    pub http_status: Option<u16>,
    pub host: String,
    pub insecure_key: bool,
}

/// Bucket an HTTP status code. 2xx is provisionally `Ok` — the caller downgrades to
/// `ServerError` if the body isn't a models list. Non-2xx never depends on a body.
fn classify_status(code: u16) -> RemoteAuthStatus {
    match code {
        401 | 403 => RemoteAuthStatus::Unauthorized,
        404 => RemoteAuthStatus::NotFound,
        200..=299 => RemoteAuthStatus::Ok,
        _ => RemoteAuthStatus::ServerError,
    }
}

/// Bucket a transport error. A TLS/cert failure is its own bucket; a timeout or a refused
/// connection is `Unreachable` — NEVER `Unauthorized` (the credential is not the cause).
fn classify_error(e: &reqwest::Error) -> RemoteAuthStatus {
    if is_tls_error(e) {
        return RemoteAuthStatus::TlsError;
    }
    RemoteAuthStatus::Unreachable
}

/// Walk the error chain for a TLS/certificate signature (reqwest surfaces the rustls/native
/// error as a source). Best-effort string match — a false negative just reads as `Unreachable`.
fn is_tls_error(e: &reqwest::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = src {
        let s = err.to_string().to_lowercase();
        if s.contains("certificate") || s.contains("tls") || s.contains("ssl") || s.contains("handshake") {
            return true;
        }
        src = err.source();
    }
    false
}

/// A key is configured but the URL isn't https/loopback → the key is WITHHELD (rule 7d never
/// transmits a credential over plain http). Surfaced so the UI can warn "use https or drop the
/// key". Pure — no network — so it's directly testable.
pub(crate) fn key_withheld_insecure(endpoint: &str, api_key: Option<&str>) -> bool {
    api_key.is_some_and(|k| !k.is_empty()) && !credential_allowed(endpoint)
}

/// Scheme + host + port ONLY — strips any `user:pass@` userinfo and the path/query, so a URL
/// with embedded credentials can never reach the report/UI/log. Unparseable → a neutral label.
pub(crate) fn host_of(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(u) => match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{}://{h}:{p}", u.scheme()),
            (Some(h), None) => format!("{}://{h}", u.scheme()),
            _ => "the configured endpoint".into(),
        },
        Err(_) => "the configured endpoint".into(),
    }
}

/// Resolve + validate a remote endpoint's credential via `GET /v1/models`, classifying the
/// outcome across the full failure space (see `RemoteAuthStatus`). The key is sent only over
/// https/loopback (rule 7d); the report leaks nothing (only a redacted host).
pub async fn probe_remote_credential(endpoint: &str, api_key: Option<&str>) -> RemoteAuthReport {
    let host = host_of(endpoint);
    let insecure_key = key_withheld_insecure(endpoint, api_key);
    if endpoint.is_empty() {
        return RemoteAuthReport { status: RemoteAuthStatus::Unconfigured, http_status: None, host, insecure_key: false };
    }
    let client = match Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return RemoteAuthReport { status: RemoteAuthStatus::Unreachable, http_status: None, host, insecure_key },
    };
    let mut req = client.get(format!("{endpoint}/v1/models"));
    if let Some(key) = api_key.filter(|k| !k.is_empty() && credential_allowed(endpoint)) {
        req = req.bearer_auth(key);
    }
    match req.send().await {
        Ok(r) => {
            let code = r.status().as_u16();
            if r.status().is_success() {
                // A 2xx is only OK if it's actually a models list — guards a proxy/misroute
                // that 200s an error page.
                let body = r.text().await.unwrap_or_default();
                let is_models = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| v.get("data").map(|d| d.is_array()))
                    .unwrap_or(false);
                let status = if is_models { RemoteAuthStatus::Ok } else { RemoteAuthStatus::ServerError };
                RemoteAuthReport { status, http_status: Some(code), host, insecure_key }
            } else {
                RemoteAuthReport { status: classify_status(code), http_status: Some(code), host, insecure_key }
            }
        }
        Err(e) => RemoteAuthReport { status: classify_error(&e), http_status: None, host, insecure_key },
    }
}

/// Probe a remote OpenAI-compatible server via `GET /v1/models` (the liveness
/// endpoint vLLM exposes), sending the bearer key when configured.
/// These servers report no version string here, so `version` stays `None`. An
/// unconfigured endpoint (empty URL) is `available: false` without any HTTP call.
pub async fn remote_health(endpoint: &str, api_key: Option<&str>) -> HealthStatus {
    if endpoint.is_empty() {
        return HealthStatus { available: false, version: None };
    }
    let client = match Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return HealthStatus { available: false, version: None },
    };
    let mut req = client.get(format!("{endpoint}/v1/models"));
    // rule 7d: only send the key over https/loopback (defense-in-depth behind the save guard).
    if let Some(key) = api_key.filter(|k| !k.is_empty() && credential_allowed(endpoint)) {
        req = req.bearer_auth(key);
    }
    match req.send().await {
        Ok(r) if r.status().is_success() => HealthStatus { available: true, version: None },
        _ => HealthStatus { available: false, version: None },
    }
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn check_vllm_health() -> HealthStatus {
    let ep = remote_config::vllm();
    remote_health(&ep.url.unwrap_or_default(), ep.api_key.as_deref()).await
}


#[cfg(feature = "gui")]
#[tauri::command]
pub async fn check_vllm_credential() -> RemoteAuthReport {
    let ep = remote_config::vllm();
    probe_remote_credential(&ep.url.unwrap_or_default(), ep.api_key.as_deref()).await
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unconfigured_endpoint_is_unavailable_without_a_network_call() {
        let h = remote_health("", None).await;
        assert!(!h.available);
        assert!(h.version.is_none());
    }

    #[tokio::test]
    async fn unreachable_endpoint_is_unavailable() {
        // Port 1 never listens; connection is refused fast.
        let h = remote_health("http://127.0.0.1:1", Some("k")).await;
        assert!(!h.available);
    }

    // ── probe_remote_credential: the full failure space, fixture-first (real round-trips) ──

    #[test]
    fn classify_status_buckets_each_code() {
        assert_eq!(classify_status(200), RemoteAuthStatus::Ok);
        assert_eq!(classify_status(401), RemoteAuthStatus::Unauthorized);
        assert_eq!(classify_status(403), RemoteAuthStatus::Unauthorized);
        assert_eq!(classify_status(404), RemoteAuthStatus::NotFound);
        assert_eq!(classify_status(500), RemoteAuthStatus::ServerError);
        assert_eq!(classify_status(502), RemoteAuthStatus::ServerError);
    }

    #[test]
    fn host_of_strips_credentials_and_path() {
        // A URL with embedded userinfo must never survive into the redacted host.
        let h = host_of("https://user:sk-secret@gpu.example.com:8000/v1/models");
        assert_eq!(h, "https://gpu.example.com:8000");
        assert!(!h.contains("sk-secret") && !h.contains("user"));
        assert_eq!(host_of("http://127.0.0.1:8000"), "http://127.0.0.1:8000");
        assert_eq!(host_of("not a url"), "the configured endpoint");
    }

    #[tokio::test]
    async fn unconfigured_makes_no_call() {
        let r = probe_remote_credential("", Some("k")).await;
        assert_eq!(r.status, RemoteAuthStatus::Unconfigured);
        assert!(r.http_status.is_none());
    }

    #[test]
    fn insecure_key_is_flagged_only_for_a_key_over_non_https_non_loopback() {
        // Remote plain http + a key → withheld → flag it (pure, no network).
        assert!(key_withheld_insecure("http://gpu.example.com:8000", Some("sk-secret")));
        // https and loopback are secure channels → NOT flagged.
        assert!(!key_withheld_insecure("https://gpu.example.com:8000", Some("sk-secret")));
        assert!(!key_withheld_insecure("http://127.0.0.1:8000", Some("sk-secret")));
        // No key → nothing to flag.
        assert!(!key_withheld_insecure("http://gpu.example.com:8000", None));
    }

    #[tokio::test]
    async fn a_down_server_is_unreachable_never_unauthorized() {
        // Real connection-refused (port 1 never listens) — NOT a stub.
        let r = probe_remote_credential("http://127.0.0.1:1", Some("k")).await;
        assert_eq!(r.status, RemoteAuthStatus::Unreachable);
    }

    /// Spawn a throwaway one-shot HTTP/1.1 server that replies with `raw` to the first request,
    /// and return its `http://127.0.0.1:PORT` base. Real socket, real round-trip — no mock lib.
    async fn spawn_once(raw: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await; // drain the request line/headers
                let _ = sock.write_all(raw.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn a_wrong_key_is_unauthorized_never_unreachable() {
        // A real 401 round-trip — the server is UP, the key is bad. The exact case that must
        // NOT read as "unreachable".
        let base = spawn_once("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n").await;
        let r = probe_remote_credential(&base, Some("bad-key")).await;
        assert_eq!(r.status, RemoteAuthStatus::Unauthorized);
        assert_eq!(r.http_status, Some(401));
    }

    #[tokio::test]
    async fn a_models_list_is_ok_but_a_200_error_body_is_not() {
        let base = spawn_once("HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"data\":[]}\r\n").await;
        assert_eq!(probe_remote_credential(&base, None).await.status, RemoteAuthStatus::Ok);
        // A 2xx that isn't a models list is ServerError, not a false Ok.
        let base2 = spawn_once("HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n{\"error\":\"nope\"}").await;
        assert_eq!(probe_remote_credential(&base2, None).await.status, RemoteAuthStatus::ServerError);
    }

    #[tokio::test]
    async fn a_wrong_path_404_is_not_found_not_unauthorized() {
        let base = spawn_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
        assert_eq!(probe_remote_credential(&base, Some("k")).await.status, RemoteAuthStatus::NotFound);
    }
}

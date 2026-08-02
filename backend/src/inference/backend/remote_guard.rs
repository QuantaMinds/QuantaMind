//! Credential-transport guard (rule 7d): a bearer key may only travel over a channel that
//! can't be sniffed. vLLM endpoints are user-supplied and are frequently plain
//! `http://<remote-gpu-ip>`, so without this a configured API key leaks in cleartext to
//! anyone on-path. Enforced twice: as a save-time guardrail (clear popup) and as a
//! defense-in-depth strip at each request site.

/// Whether a credential may be attached when calling `url`. Allowed only over `https`, or to
/// loopback (`127.0.0.0/8`, `::1`, `localhost`) where the request never crosses a network.
/// A URL we can't parse is treated as NOT allowed — fail closed.
pub fn credential_allowed(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(u) => u.scheme() == "https" || is_loopback_host(&u),
        Err(_) => false,
    }
}

fn is_loopback_host(u: &reqwest::Url) -> bool {
    match u.host_str() {
        Some(h) => {
            let h = h.trim_start_matches('[').trim_end_matches(']');
            h.eq_ignore_ascii_case("localhost")
                || h.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_always_allowed() {
        assert!(credential_allowed("https://api.example.com:8000"));
        assert!(credential_allowed("https://34.10.20.30:8000/v1"));
    }

    #[test]
    fn cleartext_http_to_remote_host_is_blocked() {
        assert!(!credential_allowed("http://34.10.20.30:8000"));
        assert!(!credential_allowed("http://example.com:30000/v1"));
    }

    #[test]
    fn loopback_http_is_allowed_no_network_to_sniff() {
        assert!(credential_allowed("http://127.0.0.1:8000"));
        assert!(credential_allowed("http://localhost:8000/v1"));
        assert!(credential_allowed("http://[::1]:8000"));
        assert!(credential_allowed("http://127.5.5.5:1234"));
    }

    #[test]
    fn unparseable_url_fails_closed() {
        assert!(!credential_allowed("not a url"));
        assert!(!credential_allowed(""));
    }
}

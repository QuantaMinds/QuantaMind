use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::remote_config::{self, RemoteEndpoint};

/// Default HTTP base for the bundled `llama-server` sidecar. It sits on **8081**
/// (NOT 8080) so a manually-launched OpenAI-compatible server on the community
/// default 8080 can't shadow it — that collision made llama's `/health` pass
/// while inference 404'd.
pub const LLAMA_SERVER: &str = "http://localhost:8081";

/// A fully-resolved endpoint: the base URL plus an optional bearer token. Local
/// backends carry no token; the remote vLLM servers may (launched with
/// `--api-key`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEndpoint {
    pub url: String,
    pub api_key: Option<String>,
}

/// Resolve a backend to its base URL + auth. The local sidecar is static; the
/// remote backends read the user-configured endpoint from `remote_config` and
/// error clearly when it isn't set — so an unconfigured run fails with "set it
/// in Settings", not an opaque connect error.
pub fn resolve(kind: BackendKind) -> AppResult<ResolvedEndpoint> {
    let local = |url: String| ResolvedEndpoint { url, api_key: None };
    Ok(match kind {
        BackendKind::LlamaCpp => local(LLAMA_SERVER.to_string()),
        BackendKind::VLlm => remote(remote_config::vllm(), "vLLM")?,
    })
}

fn remote(ep: RemoteEndpoint, label: &str) -> AppResult<ResolvedEndpoint> {
    match ep.url {
        Some(url) => Ok(ResolvedEndpoint { url, api_key: ep.api_key }),
        None => Err(AppError::Validation(format!(
            "{label} endpoint not configured — set the {label} server URL in Settings"
        ))),
    }
}

/// The base URL only, infallible. An unconfigured remote backend yields `""` — the
/// probe/discovery callers treat an unreachable URL as "unavailable", and the run
/// path uses `resolve` (which reports the misconfiguration) instead.
pub fn base_url(kind: BackendKind) -> String {
    resolve(kind).map(|r| r.url).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::backend::remote_config::CONFIG_TEST_LOCK;

    #[test]
    fn llama_uses_its_distinct_static_port() {
        assert!(base_url(BackendKind::LlamaCpp).ends_with(":8081"));
        // 8081, not the community-default 8080 a hand-launched server would take.
        assert!(!base_url(BackendKind::LlamaCpp).ends_with(":8080"));
    }

    #[test]
    fn local_backends_resolve_to_http_urls_with_no_auth() {
        for kind in [BackendKind::LlamaCpp] {
            let r = resolve(kind).expect("local always resolves");
            assert!(r.url.starts_with("http://"), "{kind:?}");
            assert!(r.api_key.is_none(), "{kind:?}");
        }
    }

    #[test]
    fn remote_backend_errors_until_configured_then_resolves_with_auth() {
        let _g = CONFIG_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        remote_config::set_vllm(None, None);
        assert!(resolve(BackendKind::VLlm).is_err(), "unconfigured vLLM must error");
        assert_eq!(base_url(BackendKind::VLlm), "", "unconfigured base_url is empty");

        remote_config::set_vllm(Some("http://gpu:8000".into()), Some("sk-1".into()));
        let r = resolve(BackendKind::VLlm).expect("configured vLLM resolves");
        assert_eq!(r.url, "http://gpu:8000");
        assert_eq!(r.api_key.as_deref(), Some("sk-1"));
        remote_config::set_vllm(None, None);
    }
}

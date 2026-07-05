use crate::errors::{AppError, AppResult};
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::remote_config::{self, RemoteEndpoint};
use crate::inference::mlx::server::mlx_endpoint::mlx_endpoint;

/// Default HTTP base for each backend. Ollama serves on its well-known port; the
/// bundled `llama-server` and `mlx_lm.server` sidecars each use a distinct port so
/// all can run side by side. llama-server is on **8081** (NOT 8080) so it can't be
/// shadowed by a manually-launched `mlx_lm.server`, whose default port is 8080 —
/// that collision made llama's `/health` pass while inference 404'd.
pub const OLLAMA: &str = "http://localhost:11434";
pub const LLAMA_SERVER: &str = "http://localhost:8081";
pub const MLX_SERVER: &str = "http://localhost:8082";

/// The whisper.cpp STT sidecar. STT is a parallel capability, not an LLM
/// `BackendKind`, so it is not part of `default_for`. Port **8093** sits clear
/// of MLX's dynamic scan range (`find_available_port` probes 8082..=8092) and
/// llama's 8081, so an STT server can coexist with either LLM sidecar without a
/// port collision rather than relying on the start-time conflict handler.
pub const WHISPER_SERVER: &str = "http://localhost:8093";

/// A fully-resolved endpoint: the base URL plus an optional bearer token. Local
/// backends carry no token; the remote vLLM/SGLang servers may (launched with
/// `--api-key`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEndpoint {
    pub url: String,
    pub api_key: Option<String>,
}

/// Resolve a backend to its base URL + auth. Local backends are static (MLX reads
/// its app-managed dynamic port); the remote backends read the user-configured
/// endpoint from `remote_config` and error clearly when it isn't set — so an
/// unconfigured run fails with "set it in Settings", not an opaque connect error.
pub fn resolve(kind: BackendKind) -> AppResult<ResolvedEndpoint> {
    let local = |url: String| ResolvedEndpoint { url, api_key: None };
    Ok(match kind {
        BackendKind::Ollama => local(OLLAMA.to_string()),
        BackendKind::LlamaCpp => local(LLAMA_SERVER.to_string()),
        BackendKind::Mlx => local(mlx_endpoint()),
        BackendKind::VLlm => remote(remote_config::vllm(), "vLLM")?,
        BackendKind::SgLang => remote(remote_config::sglang(), "SGLang")?,
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
    fn ollama_and_llama_have_distinct_static_ports() {
        assert!(base_url(BackendKind::Ollama).ends_with(":11434"));
        assert!(base_url(BackendKind::LlamaCpp).ends_with(":8081"));
        assert_ne!(base_url(BackendKind::Ollama), base_url(BackendKind::LlamaCpp));
    }

    #[test]
    fn whisper_port_is_distinct_and_clear_of_the_mlx_scan_range() {
        assert!(WHISPER_SERVER.ends_with(":8093"));
        assert!(WHISPER_SERVER.starts_with("http://"));
        for ep in [OLLAMA, LLAMA_SERVER, MLX_SERVER] {
            assert_ne!(WHISPER_SERVER, ep);
        }
        // 8093 is above MLX's dynamic probe window 8082..=8092, so the two
        // sidecars never contend for a port.
        for p in 8082..=8092 {
            assert!(!WHISPER_SERVER.ends_with(&format!(":{p}")));
        }
    }

    #[test]
    fn local_backends_resolve_to_http_urls_with_no_auth() {
        for kind in [BackendKind::Ollama, BackendKind::LlamaCpp, BackendKind::Mlx] {
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

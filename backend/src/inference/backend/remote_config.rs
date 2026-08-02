use crate::sync::MutexExt;
use std::sync::Mutex;

/// A user-configured remote OpenAI-compatible endpoint (vLLM / SGLang). Unlike the
/// localhost sidecars, these run on a remote GPU, so their URL + optional bearer
/// key come from `UserSettings`.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RemoteEndpoint {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

/// Process-globals holding the resolved remote endpoints. Mirrors
/// `inference/` can't read Tauri state, so the
/// command layer pushes settings here (on load and on every save) and the
/// dispatch path reads them via `endpoint::resolve`.
static VLLM: Mutex<RemoteEndpoint> = Mutex::new(RemoteEndpoint { url: None, api_key: None });
static SGLANG: Mutex<RemoteEndpoint> = Mutex::new(RemoteEndpoint { url: None, api_key: None });

/// Trim then drop empty strings to `None`, so a blank Settings field reads as
/// "unconfigured" rather than an empty URL that would fail deep in a request.
fn norm(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn set_vllm(url: Option<String>, api_key: Option<String>) {
    *VLLM.lock_recover() = RemoteEndpoint { url: norm(url), api_key: norm(api_key) };
}

pub fn set_sglang(url: Option<String>, api_key: Option<String>) {
    *SGLANG.lock_recover() = RemoteEndpoint { url: norm(url), api_key: norm(api_key) };
}

pub fn vllm() -> RemoteEndpoint {
    VLLM.lock_recover().clone()
}

pub fn sglang() -> RemoteEndpoint {
    SGLANG.lock_recover().clone()
}

/// Serializes tests that touch these process-globals (cargo runs them in
/// parallel within one binary).
#[cfg(test)]
pub static CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_read_round_trips_and_trims_blanks_to_none() {
        let _g = CONFIG_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_vllm(Some("  http://host:8000 ".into()), Some(" key ".into()));
        let ep = vllm();
        assert_eq!(ep.url.as_deref(), Some("http://host:8000"));
        assert_eq!(ep.api_key.as_deref(), Some("key"));

        set_vllm(Some("   ".into()), Some("".into()));
        assert_eq!(vllm(), RemoteEndpoint::default());

        set_sglang(Some("http://host:30000".into()), None);
        assert_eq!(sglang().url.as_deref(), Some("http://host:30000"));
        assert!(sglang().api_key.is_none());
        set_sglang(None, None);
    }
}

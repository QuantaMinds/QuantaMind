use crate::commands::system::health::HealthStatus;
use crate::inference::backend::remote_config;
use crate::inference::backend::remote_guard::credential_allowed;
use reqwest::Client;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// Probe a remote OpenAI-compatible server via `GET /v1/models` (the liveness
/// endpoint vLLM/SGLang both expose), sending the bearer key when configured.
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

#[tauri::command]
pub async fn check_vllm_health() -> HealthStatus {
    let ep = remote_config::vllm();
    remote_health(&ep.url.unwrap_or_default(), ep.api_key.as_deref()).await
}

#[tauri::command]
pub async fn check_sglang_health() -> HealthStatus {
    let ep = remote_config::sglang();
    remote_health(&ep.url.unwrap_or_default(), ep.api_key.as_deref()).await
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
}

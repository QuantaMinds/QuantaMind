use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// 2500ms gives room for the server to respond while it's busy loading a
// large model (which routinely pushes the response past 800ms). A
// genuinely down server still fails fast via "Connection refused", so
// the larger budget only kicks in on real load events, not for outage
// detection latency.
const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HealthStatus {
    pub available: bool,
    pub version: Option<String>,
}

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}

pub async fn probe_health(endpoint: &str) -> HealthStatus {
    let client = match Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return HealthStatus { available: false, version: None },
    };
    let resp = match client.get(format!("{endpoint}/api/version")).send().await {
        Ok(r) => r,
        Err(_) => return HealthStatus { available: false, version: None },
    };
    if !resp.status().is_success() {
        return HealthStatus { available: false, version: None };
    }
    match resp.json::<VersionResponse>().await {
        Ok(v) => HealthStatus { available: true, version: Some(v.version) },
        Err(_) => HealthStatus { available: true, version: None },
    }
}

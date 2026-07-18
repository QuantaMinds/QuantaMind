//! Ollama probe: `/api/version` (reachability + version), `/api/tags` (models),
//! `/api/show` (native tool-calling capability).

use super::ollama_models;
use crate::cli::doctor::report::{BackendDoctor, NativeFc};
use crate::commands::remote::remote_health::host_of;
use crate::commands::system::health::probe_health;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::ollama::ollama_show::probe_supports_tools;
use reqwest::Client;

pub(super) async fn probe(c: &Client, ep: &str, model: Option<&str>) -> BackendDoctor {
    let health = probe_health(ep).await;
    let models = if health.available { ollama_models(c, ep).await } else { vec![] };
    let native_fc = match (health.available, model) {
        (true, Some(m)) => {
            if probe_supports_tools(ep, m).await {
                NativeFc::Supported
            } else {
                NativeFc::Unsupported
            }
        }
        _ => NativeFc::NotProbed,
    };
    BackendDoctor {
        kind: BackendKind::Ollama,
        endpoint: host_of(ep),
        reachable: health.available,
        version: health.version,
        models,
        credential: None,
        native_fc,
    }
}

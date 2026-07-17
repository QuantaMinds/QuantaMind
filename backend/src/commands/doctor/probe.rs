//! Per-backend probing — composes the existing health/credential/capability
//! probes into one `BackendDoctor`. No new network logic: reachability, the
//! credential classifier, and the native-FC probe are all reused verbatim.

use super::report::{BackendDoctor, NativeFc};
use crate::commands::remote::remote_health::{host_of, probe_remote_credential, RemoteAuthStatus};
use crate::commands::system::health::probe_health;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::backend::remote_guard::credential_allowed;
use crate::inference::mlx::server::mlx_endpoint::mlx_endpoint;
use crate::inference::ollama::ollama_show::probe_supports_tools;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

fn client() -> Option<Client> {
    Client::builder().timeout(PROBE_TIMEOUT).build().ok()
}

/// Candidate base URLs for a backend, best-first. `--base`/`QM_BASE` always wins.
/// Otherwise we probe QuantaMind's provisioned sidecar port first, then the
/// community default a *user-launched* server would use.
fn candidates(kind: BackendKind, override_base: Option<&str>) -> Vec<String> {
    if let Some(b) = override_base.filter(|b| !b.is_empty()) {
        return vec![b.to_string()];
    }
    match kind {
        BackendKind::Ollama => vec![endpoint::OLLAMA.to_string()],
        BackendKind::LlamaCpp => vec![endpoint::LLAMA_SERVER.to_string(), "http://localhost:8080".into()],
        BackendKind::Mlx => vec![mlx_endpoint(), "http://localhost:8080".into()],
        BackendKind::VLlm => vec!["http://localhost:8000".into()],
        BackendKind::SgLang => vec!["http://localhost:30000".into()],
    }
}

#[derive(Deserialize)]
struct ModelsList {
    #[serde(default)]
    data: Vec<ModelEntry>,
}
#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// `GET /v1/models` → the served model ids, or `None` when the endpoint didn't
/// answer with a models list. The key is sent only over https/loopback (rule 7d).
async fn openai_models(c: &Client, ep: &str, key: Option<&str>) -> Option<Vec<String>> {
    let mut req = c.get(format!("{ep}/v1/models"));
    if let Some(k) = key.filter(|k| !k.is_empty() && credential_allowed(ep)) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let list: ModelsList = resp.json().await.ok()?;
    Some(list.data.into_iter().map(|m| m.id).collect())
}

#[derive(Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaTag>,
}
#[derive(Deserialize)]
struct OllamaTag {
    name: String,
}

/// Ollama's installed models via `GET /api/tags`. Empty vec on any error — the
/// caller renders "no models installed" (a loud finding), never a fabricated list.
async fn ollama_models(c: &Client, ep: &str) -> Vec<String> {
    let Ok(resp) = c.get(format!("{ep}/api/tags")).send().await else {
        return vec![];
    };
    if !resp.status().is_success() {
        return vec![];
    }
    resp.json::<OllamaTags>()
        .await
        .map(|t| t.models.into_iter().map(|m| m.name).collect())
        .unwrap_or_default()
}

fn unreachable(kind: BackendKind, ep: &str) -> BackendDoctor {
    BackendDoctor {
        kind,
        endpoint: host_of(ep),
        reachable: false,
        version: None,
        models: vec![],
        credential: None,
        native_fc: NativeFc::NotProbed,
    }
}

/// Probe one backend into a `BackendDoctor`. `model` enables Ollama's native-FC
/// capability check; `key` is the remote bearer credential (never from argv).
pub async fn probe_backend(
    kind: BackendKind,
    override_base: Option<&str>,
    model: Option<&str>,
    key: Option<&str>,
) -> BackendDoctor {
    let cands = candidates(kind, override_base);
    let Some(c) = client() else {
        return unreachable(kind, &cands[0]);
    };
    match kind {
        BackendKind::Ollama => probe_ollama(&c, &cands[0], model).await,
        BackendKind::LlamaCpp | BackendKind::Mlx => probe_local_openai(&c, kind, &cands).await,
        BackendKind::VLlm | BackendKind::SgLang => probe_remote_openai(&c, kind, &cands[0], key).await,
    }
}

/// Ollama: `/api/version` for reachability+version, `/api/tags` for models,
/// `/api/show` for the native-FC capability.
async fn probe_ollama(c: &Client, ep: &str, model: Option<&str>) -> BackendDoctor {
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

/// llama.cpp / mlx: OpenAI-compatible, no auth. First candidate that answers
/// `/v1/models` wins. No version endpoint on this path → "not available" (honest).
async fn probe_local_openai(c: &Client, kind: BackendKind, cands: &[String]) -> BackendDoctor {
    for ep in cands {
        if let Some(models) = openai_models(c, ep, None).await {
            return BackendDoctor {
                kind,
                endpoint: host_of(ep),
                reachable: true,
                version: None,
                models,
                credential: None,
                native_fc: NativeFc::NotProbed,
            };
        }
    }
    unreachable(kind, &cands[0])
}

/// vLLM / SGLang: run the credential classifier (the full Unreachable/401/insecure
/// failure space), then list models only when the credential resolved `Ok`.
async fn probe_remote_openai(c: &Client, kind: BackendKind, ep: &str, key: Option<&str>) -> BackendDoctor {
    let cred = probe_remote_credential(ep, key).await;
    let reachable = matches!(
        cred.status,
        RemoteAuthStatus::Ok | RemoteAuthStatus::Unauthorized | RemoteAuthStatus::NotFound | RemoteAuthStatus::ServerError
    );
    let models = if cred.status == RemoteAuthStatus::Ok {
        openai_models(c, ep, key).await.unwrap_or_default()
    } else {
        vec![]
    };
    BackendDoctor {
        kind,
        endpoint: cred.host.clone(),
        reachable,
        version: None,
        models,
        credential: Some(cred),
        native_fc: NativeFc::NotProbed,
    }
}

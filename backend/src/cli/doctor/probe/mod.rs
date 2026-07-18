//! Per-engine backend probing for `qm doctor`. The dispatcher + the shared HTTP
//! helpers live here; each inference engine's strategy is its own child module:
//! - [`ollama`] — native `/api/version` + `/api/tags` + `/api/show` (tools capability).
//! - [`openai_local`] — llama.cpp / MLX over the OpenAI `/v1/models` surface, no auth.
//! - [`remote`] — vLLM / SGLang, run through the credential classifier.
//!
//! No new network logic anywhere: reachability, the credential classifier, and the
//! native-FC probe are all reused verbatim from the existing modules.

use super::report::{BackendDoctor, NativeFc};
use crate::commands::remote::remote_health::host_of;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::backend::remote_guard::credential_allowed;
use crate::inference::mlx::server::mlx_endpoint::mlx_endpoint;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

mod ollama;
mod openai_local;
mod remote;

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

/// A down/unresponsive backend, with its endpoint redacted (scheme+host+port).
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

/// Probe one backend into a `BackendDoctor`, dispatching to the per-engine strategy.
/// `model` enables Ollama's native-FC check; `key` is the remote bearer credential
/// (never from argv).
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
        BackendKind::Ollama => ollama::probe(&c, &cands[0], model).await,
        BackendKind::LlamaCpp | BackendKind::Mlx => openai_local::probe(&c, kind, &cands).await,
        BackendKind::VLlm | BackendKind::SgLang => remote::probe(&c, kind, &cands[0], key).await,
    }
}

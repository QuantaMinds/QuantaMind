//! vLLM / SGLang probe: run the credential classifier (the full
//! Unreachable/401/NotFound/insecure failure space), then list models only when the
//! credential resolved `Ok`.

use super::openai_models;
use crate::cli::doctor::report::{BackendDoctor, NativeFc};
use crate::commands::remote::remote_health::{probe_remote_credential, RemoteAuthStatus};
use crate::inference::backend::backend_kind::BackendKind;
use reqwest::Client;

pub(super) async fn probe(c: &Client, kind: BackendKind, ep: &str, key: Option<&str>) -> BackendDoctor {
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

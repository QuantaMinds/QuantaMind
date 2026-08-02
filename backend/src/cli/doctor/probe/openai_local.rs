//! llama.cpp probe: OpenAI-compatible `/v1/models`, no auth. The first
//! reachable candidate port wins; there's no version endpoint on this path, so the
//! version is honestly "not available".

use super::{openai_models, unreachable};
use crate::cli::doctor::report::{BackendDoctor, NativeFc};
use crate::commands::remote::remote_health::host_of;
use crate::inference::backend::backend_kind::BackendKind;
use reqwest::Client;

pub(super) async fn probe(c: &Client, kind: BackendKind, cands: &[String]) -> BackendDoctor {
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

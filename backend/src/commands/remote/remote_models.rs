use crate::commands::storage::storage_types::InstalledModelInfo;
use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::remote_config::{self, RemoteEndpoint};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);

/// OpenAI `GET /v1/models` response. Only the model `id` is used — it is the name
/// sent in the request body. Sizes/params/quant are not exposed by these servers,
/// so those fields stay empty ("not available") rather than fabricated.
#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelObject>,
}

#[derive(Deserialize)]
struct ModelObject {
    id: String,
}

fn to_info(id: String, backend: BackendKind) -> InstalledModelInfo {
    InstalledModelInfo {
        name: id,
        size_bytes: 0,
        modified_at: String::new(),
        family: String::new(),
        parameter_size: String::new(),
        quantization: String::new(),
        backend,
        digest: String::new(),
        display_name: None,
        path: None,
    }
}

/// List the models a remote OpenAI-compatible server currently serves. An
/// unconfigured or unreachable endpoint yields an empty list (not an error),
/// matching the frontend's `Promise.allSettled` model-list contract.
pub async fn list_remote_models(ep: &RemoteEndpoint, backend: BackendKind) -> Vec<InstalledModelInfo> {
    let Some(url) = ep.url.as_deref().filter(|u| !u.is_empty()) else {
        return Vec::new();
    };
    let client = match Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut req = client.get(format!("{url}/v1/models"));
    if let Some(key) = ep.api_key.as_deref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let parsed = match req.send().await {
        Ok(r) if r.status().is_success() => r.json::<ModelsResponse>().await.ok(),
        _ => None,
    };
    parsed
        .map(|m| m.data.into_iter().map(|o| to_info(o.id, backend)).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_vllm_models() -> Result<Vec<InstalledModelInfo>, AppError> {
    Ok(list_remote_models(&remote_config::vllm(), BackendKind::VLlm).await)
}

#[tauri::command]
pub async fn list_sglang_models() -> Result<Vec<InstalledModelInfo>, AppError> {
    Ok(list_remote_models(&remote_config::sglang(), BackendKind::SgLang).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unconfigured_endpoint_yields_an_empty_list() {
        let out = list_remote_models(&RemoteEndpoint::default(), BackendKind::VLlm).await;
        assert!(out.is_empty());
    }

    #[test]
    fn model_id_maps_to_name_with_no_fabricated_metrics() {
        let info = to_info("qwen2.5-7b-instruct".into(), BackendKind::SgLang);
        assert_eq!(info.name, "qwen2.5-7b-instruct");
        assert_eq!(info.backend, BackendKind::SgLang);
        assert_eq!(info.size_bytes, 0);
        assert!(info.parameter_size.is_empty() && info.quantization.is_empty());
    }

    /// End-to-end against a REAL vLLM/SGLang server (CLAUDE.md rule 6). Exercises the
    /// production code paths — discovery, streaming generation (with thinking), and a
    /// native tool-call — not a curl. Ignored by default; run with:
    ///   QM_LIVE_URL=http://127.0.0.1:8000 QM_LIVE_BACKEND=vllm \
    ///     cargo test --lib live_remote_end_to_end -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "live: set QM_LIVE_URL (+ QM_LIVE_KEY, QM_LIVE_BACKEND=vllm|sglang)"]
    async fn live_remote_end_to_end() {
        use crate::inference::backend::backend::InferenceBackend;
        use crate::inference::generate::generate_options::GenerateOptions;
        use crate::inference::generate::generate_spec::GenerateSpec;
        use crate::inference::openai::chat_tools::chat_with_tools;
        use crate::inference::sglang::sglang_backend::SgLangBackend;
        use crate::inference::vllm::vllm_backend::VLlmBackend;
        use serde_json::json;
        use tokio_util::sync::CancellationToken;

        let url = std::env::var("QM_LIVE_URL").expect("set QM_LIVE_URL");
        let key = std::env::var("QM_LIVE_KEY").ok().filter(|k| !k.is_empty());
        let backend = match std::env::var("QM_LIVE_BACKEND").as_deref() {
            Ok("sglang") => BackendKind::SgLang,
            _ => BackendKind::VLlm,
        };
        let ep = RemoteEndpoint { url: Some(url.clone()), api_key: key.clone() };

        // 1) Discovery via GET /v1/models — the exact code the model picker runs.
        let models = list_remote_models(&ep, backend).await;
        println!("[live] {backend:?} discovered: {:?}", models.iter().map(|m| &m.name).collect::<Vec<_>>());
        assert!(!models.is_empty(), "discovery returned no models");
        assert_eq!(models[0].backend, backend);
        let model = models[0].name.clone();

        // 2) Streaming generation WITH thinking (Qwen3 is a thinking model).
        let spec = GenerateSpec {
            model: model.clone(),
            prompt: "What is 17 + 25? Reply with the number.".into(),
            system: Some("You are concise.".into()),
            options: Some(GenerateOptions { num_predict: Some(256), temperature: Some(0.2), ..Default::default() }),
            keep_alive: None,
            think: Some(true),
        };
        let mut out = String::new();
        let stats = match backend {
            BackendKind::SgLang => {
                SgLangBackend::new(url.clone(), key.clone(), model.clone())
                    .generate(&spec, CancellationToken::new(), |t| out.push_str(t)).await
            }
            _ => {
                VLlmBackend::new(url.clone(), key.clone(), model.clone())
                    .generate(&spec, CancellationToken::new(), |t| out.push_str(t)).await
            }
        }
        .expect("live generation failed");
        println!("[live] output ({} chars):\n{out}", out.len());
        println!(
            "[live] stats prompt_eval={:?} eval={:?} finish={:?}",
            stats.prompt_eval_count, stats.eval_count, stats.finish_reason
        );
        assert!(!out.trim().is_empty(), "generation streamed no tokens");
        assert!(stats.eval_count.unwrap_or(0) > 0, "usage eval_count should be populated from /v1 usage");
        assert!(stats.finish_reason.is_some(), "finish_reason should be carried through");

        // 3) Native tool-calling over the shared OpenAI tool client.
        let tools = json!([{
            "type": "function",
            "function": {
                "name": "add",
                "description": "Add two integers and return the sum",
                "parameters": {
                    "type": "object",
                    "properties": { "a": { "type": "integer" }, "b": { "type": "integer" } },
                    "required": ["a", "b"],
                },
            },
        }]);
        let res = chat_with_tools(
            &url,
            key.as_deref(),
            &model,
            "You compute by calling the provided tools.",
            "Use the add tool to add 17 and 25.",
            &tools,
            Some(GenerateOptions { num_predict: Some(256), ..Default::default() }),
        )
        .await
        .expect("live tool-call failed");
        println!(
            "[live] tool_calls: {:?}",
            res.tool_calls.iter().map(|c| (&c.name, &c.args)).collect::<Vec<_>>()
        );
        println!("[live] tool content: {:?}", res.content);
        assert!(
            !res.tool_calls.is_empty() || !res.content.trim().is_empty(),
            "native tool turn returned neither a tool_call nor content"
        );
    }
}

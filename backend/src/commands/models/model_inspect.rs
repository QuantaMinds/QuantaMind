use crate::errors::AppError;
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::backend::endpoint;
use crate::inference::ollama::ollama_show::show_model;
use crate::inference::vram_math::{kv_cache_bytes_at, KvPrecision};
use serde::Serialize;

/// Architecture dimensions needed for the KV-cache predictor, read from
/// `/api/show` `model_info`. `None` on `ModelInspect` when any key is missing or
/// the backend isn't Ollama.
#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct ModelDims {
    pub layers: u64,
    pub head_count: u64,
    pub head_count_kv: u64,
    pub embedding_length: u64,
    pub context_length: u64,
    /// `head_count_kv` was absent from `/api/show` and defaulted to `head_count`
    /// (MHA per GGUF convention). The KV-cache/VRAM figure derived from it is then a
    /// conservative overestimate for a GQA model — labelled as such, never fabricated.
    #[serde(default)]
    pub kv_estimated: bool,
}

/// Model metadata for the inspector / template guard. `available` is false for
/// non-Ollama backends (the data comes from Ollama's `/api/show`); the frontend
/// then shows "Not available — Ollama only" instead of a fabricated value.
#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct ModelInspect {
    pub available: bool,
    pub note: Option<String>,
    pub template: String,
    pub capabilities: Vec<String>,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    pub is_base_guess: bool,
    pub base_reason: Option<String>,
    pub dims: Option<ModelDims>,
}

/// Extract KV-cache dimensions from `/api/show` `model_info`. Keys are namespaced
/// by `general.architecture` (e.g. `llama.block_count`). `head_count_kv` is the one
/// tolerated absence: when missing/null it defaults to `head_count` (MHA per GGUF
/// convention) and `kv_estimated` is set — newer arches (e.g. `qwen35`) omit it, and
/// blocking on that would wrongly mark a working model unmeasured. The defaulted
/// figure OVERESTIMATES the cache for a GQA model — conservative, never optimistic.
/// The other four keys stay required → `None` (predictor falls back rather than guess).
fn dims_from_model_info(info: &serde_json::Map<String, serde_json::Value>) -> Option<ModelDims> {
    let arch = info.get("general.architecture")?.as_str()?;
    let g = |suffix: &str| info.get(&format!("{arch}.{suffix}")).and_then(|v| v.as_u64());
    let head_count = g("attention.head_count")?;
    let kv = g("attention.head_count_kv");
    Some(ModelDims {
        layers: g("block_count")?,
        head_count,
        head_count_kv: kv.unwrap_or(head_count),
        kv_estimated: kv.is_none(),
        embedding_length: g("embedding_length")?,
        context_length: g("context_length")?,
    })
}

fn has_role_markers(template: &str) -> bool {
    let l = template.to_lowercase();
    ["assistant", "<|im_start", "[inst]", "<|start_header_id", "<|user", "### instruction", "<start_of_turn>"]
        .iter()
        .any(|m| l.contains(m))
}

/// Advisory base-vs-instruct guess from the chat template + capabilities. A
/// `tools` capability or chat-role markers ⇒ instruct. Otherwise likely a base
/// (text-completion) model. Evidence is returned so the UI states *why*, never
/// an absolute claim.
pub fn classify_base(template: &str, capabilities: &[String]) -> (bool, Option<String>) {
    let has_tools = capabilities.iter().any(|c| c == "tools");
    if has_tools || has_role_markers(template) {
        return (false, None);
    }
    let first = if template.trim().is_empty() {
        "empty chat template"
    } else {
        "no chat-role markers in template"
    };
    (true, Some(format!("{first}; no 'tools' capability")))
}

/// Inspect an installed model (Ollama `/api/show`): chat template, capabilities,
/// and an advisory base-model guess. Non-Ollama backends return `available:
/// false` — the template/capabilities live in Ollama's metadata only.
#[tauri::command]
pub async fn inspect_model(
    model: String,
    backend: Option<BackendKind>,
) -> Result<ModelInspect, AppError> {
    if !matches!(backend.unwrap_or_default(), BackendKind::Ollama) {
        return Ok(ModelInspect {
            available: false,
            note: Some("Not available — Ollama only".into()),
            ..Default::default()
        });
    }
    let r = show_model(endpoint::OLLAMA, &model).await?;
    let (is_base_guess, base_reason) = classify_base(&r.template, &r.capabilities);
    let dims = dims_from_model_info(&r.model_info);
    Ok(ModelInspect {
        available: true,
        note: None,
        template: r.template,
        capabilities: r.capabilities,
        family: r.details.family,
        parameter_size: r.details.parameter_size,
        quantization: r.details.quantization_level,
        is_base_guess,
        base_reason,
        dims,
    })
}

/// Best-effort fetch of an Ollama model's KV-cache dimensions (`/api/show`).
/// `None` when Ollama is unreachable or any dimension key is missing — callers
/// (e.g. the readiness VRAM-fit check) then treat the fit as unmeasured rather
/// than guess. The home of the dims logic, reused so it isn't re-implemented.
pub async fn fetch_dims(model: &str) -> Option<ModelDims> {
    let r = show_model(endpoint::OLLAMA, model).await.ok()?;
    dims_from_model_info(&r.model_info)
}

/// Estimate the KV-cache size (bytes) for a model's dimensions at a given
/// context length. Thin wrapper over the canonical formula so the frontend has
/// one source of truth (the dims come from `inspect_model`). `precision` is the
/// cache storage type (`"f16"` / `"q8_0"` / `"q4_0"`); absent or unknown falls
/// back to f16 — the conservative baseline every pre-existing caller assumed.
#[tauri::command]
pub fn estimate_kv_cache_bytes(
    layers: u64,
    head_count: u64,
    head_count_kv: u64,
    embedding_length: u64,
    context_length: u64,
    precision: Option<String>,
) -> u64 {
    let p = precision.as_deref().and_then(KvPrecision::parse).unwrap_or_default();
    kv_cache_bytes_at(p, layers, head_count, head_count_kv, embedding_length, context_length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruct_template_is_not_base() {
        let (base, _) = classify_base("<|start_header_id|>assistant<|end_header_id|>", &["completion".into()]);
        assert!(!base);
    }

    #[test]
    fn tools_capability_alone_is_not_base() {
        let (base, _) = classify_base("", &["completion".into(), "tools".into()]);
        assert!(!base);
    }

    #[test]
    fn bare_template_no_tools_is_base_with_reason() {
        let (base, why) = classify_base("{{ .Prompt }}", &["completion".into()]);
        assert!(base);
        assert!(why.unwrap().contains("no 'tools' capability"));
    }

    #[test]
    fn empty_template_reports_empty_reason() {
        let (base, why) = classify_base("", &["completion".into()]);
        assert!(base);
        assert!(why.unwrap().contains("empty chat template"));
    }

    #[test]
    fn dims_parse_from_namespaced_model_info() {
        let info = serde_json::json!({
            "general.architecture": "llama",
            "llama.block_count": 32,
            "llama.attention.head_count": 32,
            "llama.attention.head_count_kv": 8,
            "llama.embedding_length": 4096,
            "llama.context_length": 8192
        });
        let d = dims_from_model_info(info.as_object().unwrap()).unwrap();
        assert_eq!(
            d,
            ModelDims { layers: 32, head_count: 32, head_count_kv: 8, embedding_length: 4096, context_length: 8192, kv_estimated: false }
        );
    }

    #[test]
    fn dims_none_when_a_required_key_is_missing() {
        // head_count absent ⇒ still None (head_count_kv is the only tolerated absence).
        let info = serde_json::json!({
            "general.architecture": "llama",
            "llama.block_count": 32
        });
        assert!(dims_from_model_info(info.as_object().unwrap()).is_none());
    }

    #[test]
    fn missing_kv_head_count_defaults_to_head_count_and_flags_estimate() {
        // qwen35 omits attention.head_count_kv — must still parse (conservative MHA).
        let info = serde_json::json!({
            "general.architecture": "qwen35",
            "qwen35.block_count": 32,
            "qwen35.attention.head_count": 16,
            "qwen35.attention.head_count_kv": serde_json::Value::Null,
            "qwen35.embedding_length": 4096,
            "qwen35.context_length": 262144
        });
        let d = dims_from_model_info(info.as_object().unwrap()).expect("parses despite null kv");
        assert_eq!(d.head_count_kv, 16); // defaulted to head_count
        assert!(d.kv_estimated);
    }
}

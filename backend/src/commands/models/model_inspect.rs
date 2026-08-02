use crate::commands::storage::storage_disk;
#[cfg(feature = "gui")]
use crate::errors::AppError;
#[cfg(feature = "gui")]
use crate::inference::backend::backend_kind::BackendKind;
use crate::inference::gguf::gguf::inspect_gguf;
#[cfg(feature = "gui")]
use crate::inference::vram_math::{kv_cache_bytes_at, KvPrecision};
use serde::Serialize;

/// Architecture dimensions needed for the KV-cache predictor, read from the
/// installed GGUF header. `None` on `ModelInspect` when any required key is
/// missing — the meters then say "Not available" rather than guess.
#[derive(Serialize, Default, Clone, Debug, PartialEq)]
pub struct ModelDims {
    pub layers: u64,
    pub head_count: u64,
    pub head_count_kv: u64,
    pub embedding_length: u64,
    pub context_length: u64,
    /// `head_count_kv` was absent from the GGUF header and defaulted to `head_count`
    /// (MHA per GGUF convention). The KV-cache/VRAM figure derived from it is then a
    /// conservative overestimate for a GQA model — labelled as such, never fabricated.
    #[serde(default)]
    pub kv_estimated: bool,
}

/// Model metadata for the inspector. `available` is always false now that the
/// chat template + capability list (which only a model-registry API exposed) has
/// no source: the frontend shows "Not available for this backend" rather than a
/// fabricated value. The GGUF-derived `dims` are still filled, so the
/// context-ceiling meters render real numbers.
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

/// KV-cache dimensions for a llama.cpp model, read from the installed GGUF header. The
/// llama-server API does NOT expose the transformer dims the KV-cache math needs (`/props`
/// gives only `n_ctx`; `/v1/models` gives `n_embd`/`n_params` but not head/layer counts) —
/// the GGUF header is the authoritative source. `None` when the file isn't found or a
/// required dimension is absent, so the ceiling meters say "Not available", never a guess.
/// `head_count_kv` tolerates absence (defaults to `head_count`, flagged `kv_estimated`).
fn gguf_dims(model: &str) -> Option<ModelDims> {
    let path = storage_disk::find_installed_gguf(model)?;
    dims_from_gguf_meta(&inspect_gguf(&path).ok()?)
}

/// Map GGUF header metadata to `ModelDims`. Pure (no file I/O) so the mapping unit-tests
/// directly. `block_count`/`head_count`/`embedding_length` are required → `None` if any is
/// absent; `head_count_kv` tolerates absence (defaults to `head_count`, flags `kv_estimated`).
fn dims_from_gguf_meta(m: &crate::inference::gguf::gguf::GgufMetadata) -> Option<ModelDims> {
    let head_count = m.head_count?;
    Some(ModelDims {
        layers: m.block_count?,
        head_count,
        head_count_kv: m.head_count_kv.unwrap_or(head_count),
        kv_estimated: m.head_count_kv.is_none(),
        embedding_length: m.embedding_length?,
        context_length: m.context_length.unwrap_or(0) as u64,
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

/// Inspect an installed model. The chat template + capability list came from a
/// model-registry API that no longer backs any supported backend, so `available`
/// is false and the card renders "Not available for this backend" rather than a
/// fabricated template. The KV-cache `dims` ARE filled from the installed GGUF
/// header, so the context-ceiling meters still show measured numbers.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn inspect_model(
    model: String,
    backend: Option<BackendKind>,
) -> Result<ModelInspect, AppError> {
    let _ = backend;
    Ok(ModelInspect {
        available: false,
        note: Some("Not available for this backend".into()),
        dims: gguf_dims(&model),
        ..Default::default()
    })
}

/// Best-effort KV-cache dimensions for a model, read from its installed GGUF
/// header. `None` when the file isn't found or a required dimension is absent —
/// callers (e.g. the readiness VRAM-fit check) then treat the fit as unmeasured
/// rather than guess. The home of the dims logic, reused so it isn't
/// re-implemented.
pub fn fetch_dims(model: &str) -> Option<ModelDims> {
    gguf_dims(model)
}

/// Estimate the KV-cache size (bytes) for a model's dimensions at a given
/// context length. Thin wrapper over the canonical formula so the frontend has
/// one source of truth (the dims come from `inspect_model`). `precision` is the
/// cache storage type (`"f16"` / `"q8_0"` / `"q4_0"`); absent or unknown falls
/// back to f16 — the conservative baseline every pre-existing caller assumed.
#[cfg(feature = "gui")]
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

/// The largest context this machine holds for a model at each KV-cache precision
/// (f16 / q8_0 / q4_0), plus whether the weights fit under the GPU's hard memory limit —
/// the data behind the Latency tab's context-ceiling meters. A `null` ceiling means
/// unmeasurable (rendered "Not available"), never a guess. `working_set_bytes` is the
/// machine's MEASURED Metal working-set limit (from the hardware snapshot's `GpuInfo`) on
/// Apple Silicon, `None` elsewhere; the ceilings budget against it so "fits" means fits on
/// the GPU. Omitting it falls back to the total-RAM heuristic (unchanged legacy behavior).
#[cfg(feature = "gui")]
#[tauri::command]
pub fn context_ceilings(
    layers: u64,
    head_count: u64,
    head_count_kv: u64,
    embedding_length: u64,
    model_bytes: u64,
    total_bytes: u64,
    working_set_bytes: Option<u64>,
) -> crate::commands::llama::llama_runtime::CtxCeilings {
    let dims = crate::commands::llama::llama_runtime::KvDims { layers, head_count, head_count_kv, embedding_length };
    crate::commands::llama::llama_runtime::ctx_ceilings(model_bytes, dims, total_bytes, working_set_bytes)
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

    fn gguf_meta(block: Option<u64>, head: Option<u64>, head_kv: Option<u64>, embd: Option<u64>, ctx: Option<u32>) -> crate::inference::gguf::gguf::GgufMetadata {
        crate::inference::gguf::gguf::GgufMetadata {
            architecture: "qwen2".into(),
            parameter_count: Some(7_000_000_000),
            context_length: ctx,
            quantization: Some("Q4_K_M".into()),
            family: "qwen2".into(),
            block_count: block,
            head_count: head,
            head_count_kv: head_kv,
            embedding_length: embd,
        }
    }

    #[test]
    fn gguf_dims_map_full_metadata() {
        let d = dims_from_gguf_meta(&gguf_meta(Some(28), Some(28), Some(4), Some(3584), Some(32768))).unwrap();
        assert_eq!(d.layers, 28);
        assert_eq!(d.head_count, 28);
        assert_eq!(d.head_count_kv, 4);
        assert_eq!(d.embedding_length, 3584);
        assert_eq!(d.context_length, 32768);
        assert!(!d.kv_estimated); // head_count_kv was present
    }

    #[test]
    fn gguf_dims_default_head_count_kv_when_absent() {
        let d = dims_from_gguf_meta(&gguf_meta(Some(28), Some(28), None, Some(3584), Some(32768))).unwrap();
        assert_eq!(d.head_count_kv, 28); // defaulted to head_count (MHA), overestimates for GQA
        assert!(d.kv_estimated);
    }

    #[test]
    fn gguf_dims_none_when_a_required_dim_is_missing() {
        // Missing block_count / head_count / embedding_length → unmeasured, never a guess.
        assert!(dims_from_gguf_meta(&gguf_meta(None, Some(28), Some(4), Some(3584), Some(32768))).is_none());
        assert!(dims_from_gguf_meta(&gguf_meta(Some(28), None, Some(4), Some(3584), Some(32768))).is_none());
        assert!(dims_from_gguf_meta(&gguf_meta(Some(28), Some(28), Some(4), None, Some(32768))).is_none());
    }



}
